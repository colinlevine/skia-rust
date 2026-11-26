/*
 * Safe Rust bridge for:
 *   src/core/SkColorSpace.cpp
 *   src/core/SkColorSpaceXformSteps.cpp
 * (Layer 2.1 skia-rs-color).
 */

#include "include/core/SkAlphaType.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkData.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkTypes.h"
#include "include/private/SkTemplates.h"
#include "modules/skcms/skcms.h"
#include "src/core/SkColorSpacePriv.h"
#include "src/core/SkColorSpaceXformSteps.h"
#include "src/core/SkRasterPipeline.h"
#include "src/core/SkRasterPipelineOpList.h"

#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>

static_assert(sizeof(skcms_TransferFunction) == 28);
static_assert(sizeof(skcms_Matrix3x3) == 36);
static_assert(sizeof(SkColorSpacePrimaries) == 32);
static_assert(sizeof(SkColorSpaceXformSteps::Flags) == 7);
static_assert(sizeof(SkColorSpaceXformSteps) == 132);
static_assert(offsetof(SkColorSpaceXformSteps, fSrcTF) == 8);
static_assert(offsetof(SkColorSpaceXformSteps, fDstTFInv) == 36);
static_assert(offsetof(SkColorSpaceXformSteps, fSrcToDstMatrix) == 64);
static_assert(offsetof(SkColorSpaceXformSteps, fSrcOotf) == 100);
static_assert(offsetof(SkColorSpaceXformSteps, fDstOotf) == 116);

struct SkiaRsXformStepsInput {
    skcms_TransferFunction src_tf;
    skcms_Matrix3x3 src_to_xyzd50;
    uint32_t src_tf_hash;
    uint32_t src_xyz_hash;
    int32_t src_tf_type;
    int32_t src_at;

    skcms_TransferFunction dst_tf;
    skcms_TransferFunction dst_tf_inv;
    skcms_Matrix3x3 dst_to_xyzd50;
    skcms_Matrix3x3 dst_from_xyzd50;
    uint32_t dst_tf_hash;
    uint32_t dst_xyz_hash;
    int32_t dst_tf_type;
    int32_t dst_at;
};

extern "C" {
bool skia_rs_color_space_get_cicp_primaries(uint8_t primaries, skcms_Matrix3x3* out_m);
bool skia_rs_color_space_get_cicp_from_matrix(const skcms_Matrix3x3* m, uint8_t* out_primaries);
bool skia_rs_color_space_get_cicp_transfer_fn(uint8_t transfer_characteristics,
                                              skcms_TransferFunction* out_tf);
bool skia_rs_color_space_primaries_to_xyzd50(const SkColorSpacePrimaries* p,
                                             skcms_Matrix3x3* out_m);
uint32_t skia_rs_color_space_hash_tf(const skcms_TransferFunction* tf);
uint32_t skia_rs_color_space_hash_xyz(const skcms_Matrix3x3* m);
int32_t skia_rs_color_space_canonicalize_rgb(const skcms_TransferFunction* tf,
                                             const skcms_Matrix3x3* to_xyz,
                                             skcms_TransferFunction* out_tf);
bool skia_rs_matrix3x3_invert(const skcms_Matrix3x3* src, skcms_Matrix3x3* dst);
skcms_Matrix3x3 skia_rs_matrix3x3_concat(const skcms_Matrix3x3* a, const skcms_Matrix3x3* b);
skcms_Matrix3x3 skia_rs_color_space_spin_matrix(const skcms_Matrix3x3* to_xyz);
bool skia_rs_color_space_is_srgb_tf(const skcms_TransferFunction* tf);
bool skia_rs_color_space_is_linear_tf(const skcms_TransferFunction* tf);
size_t skia_rs_color_space_serialize(const skcms_TransferFunction* tf,
                                     const skcms_Matrix3x3* to_xyz,
                                     uint8_t* out);
bool skia_rs_color_space_deserialize(const uint8_t* data, size_t len,
                                     skcms_TransferFunction* out_tf,
                                     skcms_Matrix3x3* out_xyz);

void skia_rs_color_space_xform_steps_build(const SkiaRsXformStepsInput* input,
                                           SkColorSpaceXformSteps* out);
void skia_rs_color_space_xform_steps_apply_unpremul(const SkColorSpaceXformSteps* steps,
                                                    float* rgba);
void skia_rs_color_space_xform_steps_apply_mid(const SkColorSpaceXformSteps* steps, float* rgba);
void skia_rs_color_space_xform_steps_apply_premul(const SkColorSpaceXformSteps* steps,
                                                  float* rgba);
}  // extern "C"

// ============================================================================
// SkColorSpace.cpp
// ============================================================================

namespace SkNamedPrimaries {

constexpr uint8_t kCicpIdApplicationDefined = 2;

bool GetCicp(CicpId primaries, skcms_Matrix3x3& toXYZD50) {
    return skia_rs_color_space_get_cicp_primaries(static_cast<uint8_t>(primaries), &toXYZD50);
}

bool GetCicpFromMatrix(const skcms_Matrix3x3& m, CicpId& primaries) {
    uint8_t id = 0;
    if (!skia_rs_color_space_get_cicp_from_matrix(&m, &id)) {
        return false;
    }
    primaries = static_cast<CicpId>(id);
    return true;
}

}  // namespace SkNamedPrimaries

namespace SkNamedTransferFn {

constexpr uint8_t kCicpIdApplicationDefined = 2;

bool GetCicp(SkNamedTransferFn::CicpId transfer_characteristics, skcms_TransferFunction& trfn) {
    return skia_rs_color_space_get_cicp_transfer_fn(
            static_cast<uint8_t>(transfer_characteristics), &trfn);
}

}  // namespace SkNamedTransferFn

bool SkColorSpacePrimaries::toXYZD50(skcms_Matrix3x3* toXYZ_D50) const {
    return skia_rs_color_space_primaries_to_xyzd50(this, toXYZ_D50);
}

SkColorSpace::SkColorSpace(const skcms_TransferFunction& transferFn,
                           const skcms_Matrix3x3& toXYZD50)
        : fTransferFn(transferFn), fToXYZD50(toXYZD50) {
    fTransferFnHash = skia_rs_color_space_hash_tf(&fTransferFn);
    fToXYZD50Hash = skia_rs_color_space_hash_xyz(&fToXYZD50);
}

sk_sp<SkColorSpace> SkColorSpace::MakeRGB(const skcms_TransferFunction& transferFn,
                                          const skcms_Matrix3x3& toXYZ) {
    if (skcms_TransferFunction_getType(&transferFn) == skcms_TFType_Invalid) {
        return nullptr;
    }

    skcms_TransferFunction canonicalTf;
    int32_t kind = skia_rs_color_space_canonicalize_rgb(&transferFn, &toXYZ, &canonicalTf);
    if (kind == 1) {
        return SkColorSpace::MakeSRGB();
    }
    if (kind == 2) {
        return SkColorSpace::MakeSRGBLinear();
    }
    return sk_sp<SkColorSpace>(new SkColorSpace(canonicalTf, toXYZ));
}

sk_sp<SkColorSpace> SkColorSpace::MakeCICP(SkNamedPrimaries::CicpId color_primaries,
                                           SkNamedTransferFn::CicpId transfer_characteristics) {
    skcms_TransferFunction trfn;
    if (!SkNamedTransferFn::GetCicp(transfer_characteristics, trfn)) {
        return nullptr;
    }

    skcms_Matrix3x3 toXYZD50;
    if (!SkNamedPrimaries::GetCicp(color_primaries, toXYZD50)) {
        return nullptr;
    }

    return SkColorSpace::MakeRGB(trfn, toXYZD50);
}

class SkColorSpaceSingletonFactory {
public:
    static SkColorSpace* Make(const skcms_TransferFunction& transferFn,
                              const skcms_Matrix3x3& to_xyz) {
        return new SkColorSpace(transferFn, to_xyz);
    }
};

SkColorSpace* sk_srgb_singleton() {
    static SkColorSpace* cs =
            SkColorSpaceSingletonFactory::Make(SkNamedTransferFn::kSRGB, SkNamedGamut::kSRGB);
    return cs;
}

SkColorSpace* sk_srgb_linear_singleton() {
    static SkColorSpace* cs =
            SkColorSpaceSingletonFactory::Make(SkNamedTransferFn::kLinear, SkNamedGamut::kSRGB);
    return cs;
}

sk_sp<SkColorSpace> SkColorSpace::MakeSRGB() {
    return sk_ref_sp(sk_srgb_singleton());
}

sk_sp<SkColorSpace> SkColorSpace::MakeSRGBLinear() {
    return sk_ref_sp(sk_srgb_linear_singleton());
}

void SkColorSpace::computeLazyDstFields() const {
    fLazyDstFieldsOnce([this] {
        // Invert 3x3 gamut, defaulting to sRGB if we can't.
        if (!skia_rs_matrix3x3_invert(&fToXYZD50, &fFromXYZD50)) {
            SkAssertResult(
                    skia_rs_matrix3x3_invert(&skcms_sRGB_profile()->toXYZD50, &fFromXYZD50));
        }

        // Invert transfer function, defaulting to sRGB if we can't.
        if (!skcms_TransferFunction_invert(&fTransferFn, &fInvTransferFn)) {
            fInvTransferFn = *skcms_sRGB_Inverse_TransferFunction();
        }
    });
}

bool SkColorSpace::isNumericalTransferFn(skcms_TransferFunction* coeffs) const {
    this->transferFn(coeffs);
    return skcms_TransferFunction_getType(coeffs) == skcms_TFType_sRGBish;
}

void SkColorSpace::transferFn(float gabcdef[7]) const {
    memcpy(gabcdef, &fTransferFn, 7 * sizeof(float));
}

void SkColorSpace::transferFn(skcms_TransferFunction* fn) const {
    *fn = fTransferFn;
}

void SkColorSpace::invTransferFn(skcms_TransferFunction* fn) const {
    this->computeLazyDstFields();
    *fn = fInvTransferFn;
}

bool SkColorSpace::toXYZD50(skcms_Matrix3x3* toXYZD50) const {
    *toXYZD50 = fToXYZD50;
    return true;
}

void SkColorSpace::gamutTransformTo(const SkColorSpace* dst, skcms_Matrix3x3* src_to_dst) const {
    dst->computeLazyDstFields();
    *src_to_dst = skia_rs_matrix3x3_concat(&dst->fFromXYZD50, &fToXYZD50);
}

bool SkColorSpace::isSRGB() const {
    return sk_srgb_singleton() == this;
}

bool SkColorSpace::gammaCloseToSRGB() const {
    return skia_rs_color_space_is_srgb_tf(&fTransferFn);
}

bool SkColorSpace::gammaIsLinear() const {
    return skia_rs_color_space_is_linear_tf(&fTransferFn);
}

sk_sp<SkColorSpace> SkColorSpace::makeLinearGamma() const {
    if (this->gammaIsLinear()) {
        return sk_ref_sp(const_cast<SkColorSpace*>(this));
    }
    return SkColorSpace::MakeRGB(SkNamedTransferFn::kLinear, fToXYZD50);
}

sk_sp<SkColorSpace> SkColorSpace::makeSRGBGamma() const {
    if (this->gammaCloseToSRGB()) {
        return sk_ref_sp(const_cast<SkColorSpace*>(this));
    }
    return SkColorSpace::MakeRGB(SkNamedTransferFn::kSRGB, fToXYZD50);
}

sk_sp<SkColorSpace> SkColorSpace::makeColorSpin() const {
    skcms_Matrix3x3 spun = skia_rs_color_space_spin_matrix(&fToXYZD50);
    return sk_sp<SkColorSpace>(new SkColorSpace(fTransferFn, spun));
}

void SkColorSpace::toProfile(skcms_ICCProfile* profile) const {
    skcms_Init(profile);
    skcms_SetTransferFunction(profile, &fTransferFn);
    skcms_SetXYZD50(profile, &fToXYZD50);

    switch (skcms_TransferFunction_getType(&fTransferFn)) {
        case skcms_TFType_PQ:
        case skcms_TFType_PQish:
            profile->has_CICP = true;
            profile->CICP.transfer_characteristics =
                    static_cast<uint8_t>(SkNamedTransferFn::CicpId::kPQ);
            break;
        case skcms_TFType_HLG:
        case skcms_TFType_HLGish:
            profile->has_CICP = true;
            profile->CICP.transfer_characteristics =
                    static_cast<uint8_t>(SkNamedTransferFn::CicpId::kHLG);
            break;
        default:
            break;
    }
    if (profile->has_CICP) {
        profile->CICP.matrix_coefficients = 0;
        profile->CICP.video_full_range_flag = 1;
        SkNamedPrimaries::CicpId primaries_id = SkNamedPrimaries::CicpId::kRec709;
        if (SkNamedPrimaries::GetCicpFromMatrix(fToXYZD50, primaries_id)) {
            profile->CICP.color_primaries = static_cast<uint8_t>(primaries_id);
        } else {
            profile->CICP.color_primaries = SkNamedPrimaries::kCicpIdApplicationDefined;
        }
    }
}

sk_sp<SkColorSpace> SkColorSpace::Make(const skcms_ICCProfile& profile) {
    bool use_cicp = profile.has_CICP && profile.CICP.matrix_coefficients == 0 &&
                    profile.CICP.video_full_range_flag == 1;
    auto cicp_color_primaries =
            static_cast<SkNamedPrimaries::CicpId>(profile.CICP.color_primaries);
    auto cicp_transfer_characteristics =
            static_cast<SkNamedTransferFn::CicpId>(profile.CICP.transfer_characteristics);

    if (use_cicp && cicp_color_primaries == SkNamedPrimaries::CicpId::kRec709 &&
        cicp_transfer_characteristics == SkNamedTransferFn::CicpId::kIEC61966_2_4) {
        return SkColorSpace::MakeSRGB();
    } else if (skcms_ApproximatelyEqualProfiles(&profile, skcms_sRGB_profile())) {
        return SkColorSpace::MakeSRGB();
    }

    skcms_Matrix3x3 toXYZD50;
    bool hasSetToXYZD50 = false;
    if (use_cicp) {
        if (SkNamedPrimaries::GetCicp(cicp_color_primaries, toXYZD50)) {
            hasSetToXYZD50 = true;
        } else if (profile.CICP.color_primaries != SkNamedPrimaries::kCicpIdApplicationDefined) {
            return nullptr;
        }
    }
    if (profile.has_toXYZD50 && !hasSetToXYZD50) {
        skcms_Matrix3x3 inv;
        toXYZD50 = profile.toXYZD50;
        if (skia_rs_matrix3x3_invert(&toXYZD50, &inv)) {
            hasSetToXYZD50 = true;
        }
    }
    if (!hasSetToXYZD50) {
        return nullptr;
    }

    skcms_TransferFunction trfn;
    bool hasSetTrfn = false;
    if (use_cicp) {
        if (SkNamedTransferFn::GetCicp(cicp_transfer_characteristics, trfn)) {
            hasSetTrfn = true;
        } else if (profile.CICP.transfer_characteristics !=
                   SkNamedTransferFn::kCicpIdApplicationDefined) {
            return nullptr;
        }
    }
    if (profile.has_trc && !hasSetTrfn) {
        const skcms_Curve* trc = profile.trc;
        if (trc[0].table_entries == 0 && trc[1].table_entries == 0 && trc[2].table_entries == 0 &&
            0 == memcmp(&trc[0].parametric, &trc[1].parametric, sizeof(trc[0].parametric)) &&
            0 == memcmp(&trc[0].parametric, &trc[2].parametric, sizeof(trc[0].parametric))) {
            trfn = profile.trc[0].parametric;
            hasSetTrfn = true;
        } else {
            if (skcms_TRCs_AreApproximateInverse(&profile,
                                                 skcms_sRGB_Inverse_TransferFunction())) {
                trfn = SkNamedTransferFn::kSRGB;
                hasSetTrfn = true;
            }
        }
    }
    if (!hasSetTrfn) {
        return nullptr;
    }

    return SkColorSpace::MakeRGB(trfn, toXYZD50);
}

size_t SkColorSpace::writeToMemory(void* memory) const {
    return skia_rs_color_space_serialize(&fTransferFn, &fToXYZD50, static_cast<uint8_t*>(memory));
}

sk_sp<SkData> SkColorSpace::serialize() const {
    sk_sp<SkData> data = SkData::MakeUninitialized(this->writeToMemory(nullptr));
    this->writeToMemory(data->writable_data());
    return data;
}

sk_sp<SkColorSpace> SkColorSpace::Deserialize(const void* data, size_t length) {
    skcms_TransferFunction transferFn;
    skcms_Matrix3x3 toXYZ;
    if (!skia_rs_color_space_deserialize(
                static_cast<const uint8_t*>(data), length, &transferFn, &toXYZ)) {
        return nullptr;
    }
    return SkColorSpace::MakeRGB(transferFn, toXYZ);
}

bool SkColorSpace::Equals(const SkColorSpace* x, const SkColorSpace* y) {
    if (x == y) {
        return true;
    }
    if (!x || !y) {
        return false;
    }
    if (x->hash() == y->hash()) {
#if defined(SK_DEBUG)
        auto equiv = [](float X, float Y) { return (X == Y) || (std::isnan(X) && std::isnan(Y)); };
        for (int i = 0; i < 7; i++) {
            float X = (&x->fTransferFn.g)[i];
            float Y = (&y->fTransferFn.g)[i];
            SkASSERTF(equiv(X, Y), "Hash collision at tf[%d], !equiv(%g,%g)\n", i, X, Y);
        }
        for (int r = 0; r < 3; r++) {
            for (int c = 0; c < 3; c++) {
                float X = x->fToXYZD50.vals[r][c];
                float Y = y->fToXYZD50.vals[r][c];
                SkASSERTF(equiv(X, Y),
                          "Hash collision at toXYZD50[%d][%d], !equiv(%g,%g)\n",
                          r, c, X, Y);
            }
        }
#endif
        return true;
    }
    return false;
}

// ============================================================================
// SkColorSpaceXformSteps.cpp
// ============================================================================

SkColorSpaceXformSteps::SkColorSpaceXformSteps(const SkColorSpace* src, SkAlphaType srcAT,
                                               const SkColorSpace* dst, SkAlphaType dstAT) {
    if (dstAT == kOpaque_SkAlphaType) {
        dstAT = srcAT;
    }
    if (!src) {
        src = sk_srgb_singleton();
    }
    if (!dst) {
        dst = src;
    }
    if (src->hash() == dst->hash() && srcAT == dstAT) {
        SkASSERT(SkColorSpace::Equals(src, dst));
        return;
    }

    SkiaRsXformStepsInput input = {};
    src->transferFn(&input.src_tf);
    src->toXYZD50(&input.src_to_xyzd50);
    input.src_tf_hash = src->transferFnHash();
    input.src_xyz_hash = src->toXYZD50Hash();
    skcms_TFType srcTfType = skcms_TransferFunction_getType(&input.src_tf);
    input.src_tf_type = static_cast<int32_t>(srcTfType);
    input.src_at = static_cast<int32_t>(srcAT);

    dst->transferFn(&input.dst_tf);
    dst->toXYZD50(&input.dst_to_xyzd50);
    if (!skia_rs_matrix3x3_invert(&input.dst_to_xyzd50, &input.dst_from_xyzd50)) {
        SkAssertResult(skia_rs_matrix3x3_invert(&skcms_sRGB_profile()->toXYZD50,
                                                &input.dst_from_xyzd50));
    }
    input.dst_tf_hash = dst->transferFnHash();
    input.dst_xyz_hash = dst->toXYZD50Hash();
    skcms_TFType dstTfType = skcms_TransferFunction_getType(&input.dst_tf);
    input.dst_tf_type = static_cast<int32_t>(dstTfType);
    input.dst_at = static_cast<int32_t>(dstAT);

    static constexpr skcms_TransferFunction kPQish = {
        -2.0f, -107 / 128.0f, 1.0f, 32 / 2523.0f, 2413 / 128.0f, -2392 / 128.0f, 8192 / 1305.0f
    };
    static constexpr skcms_TransferFunction kHLGish = {
        -3.0f, 2.0f, 2.0f, 1 / 0.17883277f, 0.28466892f, 0.55991073f, 0.0f
    };

    switch (dstTfType) {
        case skcms_TFType_PQ:
            input.dst_tf_inv = kPQish;
            skcms_TransferFunction_invert(&input.dst_tf_inv, &input.dst_tf_inv);
            break;
        case skcms_TFType_HLG:
            input.dst_tf_inv = kHLGish;
            input.dst_tf_inv.f = 1 / 12.f - 1.f;
            skcms_TransferFunction_invert(&input.dst_tf_inv, &input.dst_tf_inv);
            break;
        default:
            if (!skia_rs_color_space_is_linear_tf(&input.dst_tf)) {
                dst->invTransferFn(&input.dst_tf_inv);
            }
            break;
    }

    skia_rs_color_space_xform_steps_build(&input, this);
}

void SkColorSpaceXformSteps::apply(float* rgba) const {
    skia_rs_color_space_xform_steps_apply_unpremul(this, rgba);
    if (this->fFlags.linearize) {
        rgba[0] = skcms_TransferFunction_eval(&fSrcTF, rgba[0]);
        rgba[1] = skcms_TransferFunction_eval(&fSrcTF, rgba[1]);
        rgba[2] = skcms_TransferFunction_eval(&fSrcTF, rgba[2]);
    }
    skia_rs_color_space_xform_steps_apply_mid(this, rgba);
    if (this->fFlags.encode) {
        rgba[0] = skcms_TransferFunction_eval(&fDstTFInv, rgba[0]);
        rgba[1] = skcms_TransferFunction_eval(&fDstTFInv, rgba[1]);
        rgba[2] = skcms_TransferFunction_eval(&fDstTFInv, rgba[2]);
    }
    skia_rs_color_space_xform_steps_apply_premul(this, rgba);
}

void SkColorSpaceXformSteps::apply(SkRasterPipeline* p) const {
    if (this->fFlags.unpremul)        { p->append(SkRasterPipelineOp::unpremul); }
    if (this->fFlags.linearize)       { p->appendTransferFunction(fSrcTF); }
    if (this->fFlags.src_ootf)        { p->append(SkRasterPipelineOp::ootf, fSrcOotf); }
    if (this->fFlags.gamut_transform) { p->append(SkRasterPipelineOp::matrix_3x3, &fSrcToDstMatrix); }
    if (this->fFlags.dst_ootf)        { p->append(SkRasterPipelineOp::ootf, fDstOotf); }
    if (this->fFlags.encode)          { p->appendTransferFunction(fDstTFInv); }
    if (this->fFlags.premul)          { p->append(SkRasterPipelineOp::premul); }
}
