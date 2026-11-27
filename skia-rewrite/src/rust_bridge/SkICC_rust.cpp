/*
 * Safe Rust bridge for:
 *   src/encode/SkICC.cpp
 * (Phase 2.2 skia-rs-codec-meta).
 */

#include "include/core/SkColorSpace.h"
#include "include/core/SkData.h"
#include "include/encode/SkICC.h"
#include "include/private/SkHdrMetadata.h"
#include "modules/skcms/skcms.h"

#include <cstddef>
#include <cstdint>
#include <cstring>

namespace {

struct SkiaRsMetaBytes {
    uint8_t* ptr;
    size_t len;
    size_t cap;
};

struct SkiaRsIccCurve {
    uint32_t table_entries;
    skcms_TransferFunction parametric;
    const uint8_t* table_16;
};

struct SkiaRsIccLutStage {
    SkiaRsIccCurve b_curves[3];
    bool has_a_curves;
    SkiaRsIccCurve a_curves[3];
    bool has_grid;
    uint8_t grid_points[3];
    const uint8_t* grid_16;
    bool has_matrix;
    SkiaRsIccCurve m_curves[3];
    float matrix[3][4];
};

struct SkiaRsIccProfileDesc {
    uint32_t data_color_space;
    uint32_t pcs;
    bool has_to_xyzd50;
    skcms_Matrix3x3 to_xyzd50;
    bool has_trc;
    SkiaRsIccCurve trc[3];
    bool trc1_same_as_0;
    bool trc2_same_as_1;
    bool has_cicp;
    uint8_t cicp[4];
    bool has_hagc;
    const uint8_t* hagc_ptr;
    size_t hagc_len;
    bool has_a2b;
    SkiaRsIccLutStage a2b;
    bool has_b2a;
    SkiaRsIccLutStage b2a;
};

extern "C" {
void skia_rs_codec_meta_free_bytes(SkiaRsMetaBytes buf);
void skia_rs_icc_write_profile(const SkiaRsIccProfileDesc* profile,
                               const char* desc_cstr,
                               SkiaRsMetaBytes* out_bytes);
void skia_rs_icc_write_from_color_space(const skcms_TransferFunction* fn_tf,
                                        const skcms_Matrix3x3* to_xyzd50,
                                        const uint8_t* agtm_ptr,
                                        size_t agtm_len,
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

SkiaRsIccCurve curve_to_ffi(const skcms_Curve& c) {
    SkiaRsIccCurve out{};
    out.table_entries = c.table_entries;
    if (c.table_entries > 0) {
        out.table_16 = c.table_16;
    } else {
        out.parametric = c.parametric;
    }
    return out;
}

}  // namespace

sk_sp<SkData> SkWriteICCProfile(const skcms_ICCProfile* profile, const char* desc) {
    SkiaRsIccProfileDesc raw{};
    raw.data_color_space = profile->data_color_space;
    raw.pcs = profile->pcs;

    raw.has_to_xyzd50 = profile->has_toXYZD50;
    if (profile->has_toXYZD50) {
        raw.to_xyzd50 = profile->toXYZD50;
    }

    raw.has_trc = profile->has_trc;
    if (profile->has_trc) {
        raw.trc[0] = curve_to_ffi(profile->trc[0]);
        raw.trc[1] = curve_to_ffi(profile->trc[1]);
        raw.trc[2] = curve_to_ffi(profile->trc[2]);
        raw.trc1_same_as_0 =
                std::memcmp(&profile->trc[1], &profile->trc[0], sizeof(profile->trc[0])) == 0;
        raw.trc2_same_as_1 =
                std::memcmp(&profile->trc[2], &profile->trc[1], sizeof(profile->trc[1])) == 0;
    }

    raw.has_cicp = profile->has_CICP;
    if (profile->has_CICP) {
        raw.cicp[0] = profile->CICP.color_primaries;
        raw.cicp[1] = profile->CICP.transfer_characteristics;
        raw.cicp[2] = profile->CICP.matrix_coefficients;
        raw.cicp[3] = profile->CICP.video_full_range_flag;
    }

    raw.has_hagc = profile->has_HAGC;
    if (profile->has_HAGC) {
        raw.hagc_ptr = profile->HAGC.buffer;
        raw.hagc_len = profile->HAGC.size;
    }

    raw.has_a2b = profile->has_A2B;
    if (profile->has_A2B) {
        const auto& a2b = profile->A2B;
        SkASSERT(a2b.output_channels == 3);
        for (int i = 0; i < 3; ++i) {
            raw.a2b.b_curves[i] = curve_to_ffi(a2b.output_curves[i]);
        }
        if (a2b.input_channels) {
            raw.a2b.has_a_curves = true;
            raw.a2b.has_grid = true;
            for (int i = 0; i < 3; ++i) {
                raw.a2b.a_curves[i] = curve_to_ffi(a2b.input_curves[i]);
                raw.a2b.grid_points[i] = a2b.grid_points[i];
            }
            raw.a2b.grid_16 = a2b.grid_16;
        }
        if (a2b.matrix_channels) {
            raw.a2b.has_matrix = true;
            for (int i = 0; i < 3; ++i) {
                raw.a2b.m_curves[i] = curve_to_ffi(a2b.matrix_curves[i]);
                for (int j = 0; j < 4; ++j) {
                    raw.a2b.matrix[i][j] = a2b.matrix.vals[i][j];
                }
            }
        }
    }

    raw.has_b2a = profile->has_B2A;
    if (profile->has_B2A) {
        const auto& b2a = profile->B2A;
        SkASSERT(b2a.input_channels == 3);
        for (int i = 0; i < 3; ++i) {
            raw.b2a.b_curves[i] = curve_to_ffi(b2a.input_curves[i]);
        }
        if (b2a.output_channels) {
            raw.b2a.has_a_curves = true;
            raw.b2a.has_grid = true;
            for (int i = 0; i < 3; ++i) {
                raw.b2a.a_curves[i] = curve_to_ffi(b2a.output_curves[i]);
                raw.b2a.grid_points[i] = b2a.grid_points[i];
            }
            raw.b2a.grid_16 = b2a.grid_16;
        }
        if (b2a.matrix_channels) {
            raw.b2a.has_matrix = true;
            for (int i = 0; i < 3; ++i) {
                raw.b2a.m_curves[i] = curve_to_ffi(b2a.matrix_curves[i]);
                for (int j = 0; j < 4; ++j) {
                    raw.b2a.matrix[i][j] = b2a.matrix.vals[i][j];
                }
            }
        }
    }

    SkiaRsMetaBytes out{};
    skia_rs_icc_write_profile(&raw, desc, &out);
    return take_meta_bytes(out);
}

sk_sp<SkData> SkWriteICCProfile(const SkColorSpace* colorSpace,
                                const skhdr::Metadata* hdrMetadata) {
    if (!colorSpace) {
        return nullptr;
    }
    skcms_TransferFunction fn;
    skcms_Matrix3x3 toXYZD50;
    colorSpace->transferFn(&fn);
    colorSpace->toXYZD50(&toXYZD50);

    sk_sp<const SkData> agtm;
    if (hdrMetadata) {
        agtm = hdrMetadata->getSerializedAgtm();
    }

    SkiaRsMetaBytes out{};
    skia_rs_icc_write_from_color_space(&fn,
                                       &toXYZD50,
                                       (agtm && !agtm->empty()) ? agtm->bytes() : nullptr,
                                       (agtm && !agtm->empty()) ? agtm->size() : 0,
                                       &out);
    return take_meta_bytes(out);
}

sk_sp<SkData> SkWriteICCProfile(const skcms_TransferFunction& fn,
                                const skcms_Matrix3x3& toXYZD50) {
    return SkWriteICCProfile(SkColorSpace::MakeRGB(fn, toXYZD50).get(), nullptr);
}
