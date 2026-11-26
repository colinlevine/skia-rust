/*
 * Safe Rust bridge for:
 *   src/core/SkColor.cpp
 *   src/core/SkUnPreMultiply.cpp
 *   src/core/SkColorTable.cpp
 *   src/core/SkMasks.cpp
 *   src/core/SkImageInfo.cpp
 *   src/core/SkBlendMode.cpp
 *   src/core/SkBlendModeBlender.cpp
 * (Layer 2.1 skia-rs-color).
 */

#include "include/core/SkBlendMode.h"
#include "include/core/SkBlender.h"
#include "include/core/SkColor.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkColorTable.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkPaint.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkUnPreMultiply.h"
#include "include/private/SkAssert.h"
#include "include/private/chromium/SkPMColor.h"
#include "src/core/SkBlendModeBlender.h"
#include "src/core/SkBlendModePriv.h"
#include "src/core/SkColorData.h"
#include "src/core/SkColorPriv.h"
#include "src/core/SkEffectPriv.h"
#include "src/core/SkImageInfoPriv.h"
#include "src/core/SkMasks.h"
#include "src/core/SkNoDestructor.h"
#include "src/core/SkRasterPipeline.h"
#include "src/core/SkRasterPipelineOpContexts.h"
#include "src/core/SkRasterPipelineOpList.h"
#include "src/core/SkReadBuffer.h"
#include "src/core/SkWriteBuffer.h"

#include <cstddef>
#include <cstdint>
#include <optional>
#include <utility>

static_assert(sizeof(SkColor4f) == 16);
static_assert(sizeof(SkPMColor4f) == 16);
static_assert(sizeof(SkMasks::MaskInfo) == 12);
static_assert(sizeof(SkMasks::InputMasks) == 16);
static_assert(sizeof(SkMasks) == 48);

#if defined(SK_PMCOLOR_IS_BGRA)
static constexpr bool kPMColorIsBGRA = true;
#else
static constexpr bool kPMColorIsBGRA = false;
#endif

extern "C" {
// SkColor
uint32_t skia_rs_color_premultiply_argb(uint32_t a, uint32_t r, uint32_t g, uint32_t b,
                                        bool is_bgra);
uint32_t skia_rs_color_premultiply_color(uint32_t c, bool is_bgra);
uint32_t skia_rs_pmcolor_set_argb(uint8_t a, uint8_t r, uint8_t g, uint8_t b, bool is_bgra);
uint8_t skia_rs_pmcolor_get_a(uint32_t c);
uint8_t skia_rs_pmcolor_get_r(uint32_t c, bool is_bgra);
uint8_t skia_rs_pmcolor_get_g(uint32_t c);
uint8_t skia_rs_pmcolor_get_b(uint32_t c, bool is_bgra);
void skia_rs_color_rgb_to_hsv(uint32_t r, uint32_t g, uint32_t b, float* out_hsv);
uint32_t skia_rs_color_hsv_to_color(uint32_t a, const float* hsv);
SkColor4f skia_rs_color4f_from_color(uint32_t bgra);
uint32_t skia_rs_color4f_to_sk_color(const SkColor4f* c);
uint32_t skia_rs_color4f_to_bytes_rgba(const SkColor4f* c);
SkColor4f skia_rs_color4f_from_bytes_rgba(uint32_t c);
SkPMColor4f skia_rs_pmcolor4f_from_pmcolor(uint32_t c, bool is_bgra);
void skia_rs_color_convert_to_4f(const uint32_t* src, SkColor4f* dst, size_t count);

// SkUnPreMultiply
uint32_t skia_rs_unpremultiply_pmcolor_to_color(uint32_t c, bool is_bgra);

// SkColorTable
bool skia_rs_color_table_fill(const uint8_t* table_a, const uint8_t* table_r,
                              const uint8_t* table_g, const uint8_t* table_b,
                              uint8_t* out_argb);

// SkMasks
bool skia_rs_masks_create(SkMasks::InputMasks masks, int bytes_per_pixel, SkMasks* out);
uint8_t skia_rs_masks_get_red(const SkMasks* m, uint32_t pixel);
uint8_t skia_rs_masks_get_green(const SkMasks* m, uint32_t pixel);
uint8_t skia_rs_masks_get_blue(const SkMasks* m, uint32_t pixel);
uint8_t skia_rs_masks_get_alpha(const SkMasks* m, uint32_t pixel);

// SkImageInfo
int skia_rs_color_type_bytes_per_pixel(int ct);
bool skia_rs_color_type_is_always_opaque(int ct);
size_t skia_rs_color_type_compute_offset(int ct, int x, int y, size_t row_bytes);
size_t skia_rs_image_info_compute_byte_size(int width, int height, int ct, size_t row_bytes);
bool skia_rs_color_type_validate_alpha_type(int ct, int at, int* canonical_out);

// SkBlendMode & SkBlendModeBlender
bool skia_rs_blend_mode_should_pre_scale_coverage(int mode, bool rgb_coverage);
bool skia_rs_blend_mode_supports_coverage_as_alpha(int mode);
bool skia_rs_blend_mode_as_coeff(int mode, int* src_out, int* dst_out);
const char* skia_rs_blend_mode_name(int mode);
int skia_rs_blend_mode_check_fast_path(bool has_mode, int mode, bool just_solid_color,
                                       bool dst_is_opaque);
bool skia_rs_blend_mode_apply_fast(int mode, const SkPMColor4f* src, const SkPMColor4f* dst,
                                   SkPMColor4f* out);
bool skia_rs_blend_mode_affects_transparent_black(bool has_mode, int mode);
}  // extern "C"

// ============================================================================
// SkColor.cpp
// ============================================================================

SkPMColor SkPreMultiplyARGB(U8CPU a, U8CPU r, U8CPU g, U8CPU b) {
    return skia_rs_color_premultiply_argb(a, r, g, b, kPMColorIsBGRA);
}

SkPMColor SkPreMultiplyColor(SkColor c) {
    return skia_rs_color_premultiply_color(c, kPMColorIsBGRA);
}

SkPMColor SkPMColorSetARGB(uint8_t a, uint8_t r, uint8_t g, uint8_t b) {
    return skia_rs_pmcolor_set_argb(a, r, g, b, kPMColorIsBGRA);
}

SkAlpha SkPMColorGetA(SkPMColor c) {
    return skia_rs_pmcolor_get_a(c);
}

uint8_t SkPMColorGetR(SkPMColor c) {
    return skia_rs_pmcolor_get_r(c, kPMColorIsBGRA);
}

uint8_t SkPMColorGetG(SkPMColor c) {
    return skia_rs_pmcolor_get_g(c);
}

uint8_t SkPMColorGetB(SkPMColor c) {
    return skia_rs_pmcolor_get_b(c, kPMColorIsBGRA);
}

void SkRGBToHSV(U8CPU r, U8CPU g, U8CPU b, SkScalar hsv[3]) {
    SkASSERT(hsv);
    skia_rs_color_rgb_to_hsv(r, g, b, hsv);
}

SkColor SkHSVToColor(U8CPU a, const SkScalar hsv[3]) {
    SkASSERT(hsv);
    return skia_rs_color_hsv_to_color(a, hsv);
}

template <>
SkColor4f SkColor4f::FromColor(SkColor bgra) {
    return skia_rs_color4f_from_color(bgra);
}

template <>
SkColor SkColor4f::toSkColor() const {
    return skia_rs_color4f_to_sk_color(this);
}

template <>
uint32_t SkColor4f::toBytes_RGBA() const {
    return skia_rs_color4f_to_bytes_rgba(this);
}

template <>
SkColor4f SkColor4f::FromBytes_RGBA(uint32_t c) {
    return skia_rs_color4f_from_bytes_rgba(c);
}

template <>
SkPMColor4f SkPMColor4f::FromPMColor(SkPMColor c) {
    return skia_rs_pmcolor4f_from_pmcolor(c, kPMColorIsBGRA);
}

template <>
uint32_t SkPMColor4f::toBytes_RGBA() const {
    return skia_rs_color4f_to_bytes_rgba(reinterpret_cast<const SkColor4f*>(this));
}

template <>
SkPMColor4f SkPMColor4f::FromBytes_RGBA(uint32_t c) {
    SkColor4f tmp = skia_rs_color4f_from_bytes_rgba(c);
    return {tmp.fR, tmp.fG, tmp.fB, tmp.fA};
}

SkColorConverter::SkColorConverter(SkSpan<const SkColor> src) {
    fColors4f.resize(src.size());
    skia_rs_color_convert_to_4f(src.data(), fColors4f.data(), src.size());
}

// ============================================================================
// SkUnPreMultiply.cpp
// ============================================================================

SkColor SkUnPreMultiply::PMColorToColor(SkPMColor c) {
    return skia_rs_unpremultiply_pmcolor_to_color(c, kPMColorIsBGRA);
}

const uint32_t SkUnPreMultiply::gTable[] = {
    0x00000000, 0xFF000000, 0x7F800000, 0x55000000, 0x3FC00000, 0x33000000, 0x2A800000, 0x246DB6DB,
    0x1FE00000, 0x1C555555, 0x19800000, 0x172E8BA3, 0x15400000, 0x139D89D9, 0x1236DB6E, 0x11000000,
    0x0FF00000, 0x0F000000, 0x0E2AAAAB, 0x0D6BCA1B, 0x0CC00000, 0x0C249249, 0x0B9745D1, 0x0B1642C8,
    0x0AA00000, 0x0A333333, 0x09CEC4EC, 0x0971C71C, 0x091B6DB7, 0x08CB08D4, 0x08800000, 0x0839CE74,
    0x07F80000, 0x07BA2E8C, 0x07800000, 0x07492492, 0x07155555, 0x06E45307, 0x06B5E50D, 0x0689D89E,
    0x06600000, 0x063831F4, 0x06124925, 0x05EE23B9, 0x05CBA2E9, 0x05AAAAAB, 0x058B2164, 0x056CEFA9,
    0x05500000, 0x05343EB2, 0x0519999A, 0x05000000, 0x04E76276, 0x04CFB2B8, 0x04B8E38E, 0x04A2E8BA,
    0x048DB6DB, 0x0479435E, 0x0465846A, 0x045270D0, 0x04400000, 0x042E29F8, 0x041CE73A, 0x040C30C3,
    0x03FC0000, 0x03EC4EC5, 0x03DD1746, 0x03CE540F, 0x03C00000, 0x03B21643, 0x03A49249, 0x03976FC6,
    0x038AAAAB, 0x037E3F20, 0x03722983, 0x03666666, 0x035AF287, 0x034FCACE, 0x0344EC4F, 0x033A5441,
    0x03300000, 0x0325ED09, 0x031C18FA, 0x0312818B, 0x03092492, 0x03000000, 0x02F711DC, 0x02EE5847,
    0x02E5D174, 0x02DD7BAF, 0x02D55555, 0x02CD5CD6, 0x02C590B2, 0x02BDEF7C, 0x02B677D4, 0x02AF286C,
    0x02A80000, 0x02A0FD5C, 0x029A1F59, 0x029364D9, 0x028CCCCD, 0x0286562E, 0x02800000, 0x0279C952,
    0x0273B13B, 0x026DB6DB, 0x0267D95C, 0x026217ED, 0x025C71C7, 0x0256E62A, 0x0251745D, 0x024C1BAD,
    0x0246DB6E, 0x0241B2F9, 0x023CA1AF, 0x0237A6F5, 0x0232C235, 0x022DF2DF, 0x02293868, 0x02249249,
    0x02200000, 0x021B810F, 0x021714FC, 0x0212BB51, 0x020E739D, 0x020A3D71, 0x02061862, 0x02020408,
    0x01FE0000, 0x01FA0BE8, 0x01F62762, 0x01F25214, 0x01EE8BA3, 0x01EAD3BB, 0x01E72A08, 0x01E38E39,
    0x01E00000, 0x01DC7F11, 0x01D90B21, 0x01D5A3EA, 0x01D24925, 0x01CEFA8E, 0x01CBB7E3, 0x01C880E5,
    0x01C55555, 0x01C234F7, 0x01BF1F90, 0x01BC14E6, 0x01B914C2, 0x01B61EED, 0x01B33333, 0x01B05161,
    0x01AD7943, 0x01AAAAAB, 0x01A7E567, 0x01A5294A, 0x01A27627, 0x019FCBD2, 0x019D2A20, 0x019A90E8,
    0x01980000, 0x01957741, 0x0192F685, 0x01907DA5, 0x018E0C7D, 0x018BA2E9, 0x018940C5, 0x0186E5F1,
    0x01849249, 0x018245AE, 0x01800000, 0x017DC11F, 0x017B88EE, 0x0179574E, 0x01772C23, 0x01750750,
    0x0172E8BA, 0x0170D045, 0x016EBDD8, 0x016CB157, 0x016AAAAB, 0x0168A9B9, 0x0166AE6B, 0x0164B8A8,
    0x0162C859, 0x0160DD68, 0x015EF7BE, 0x015D1746, 0x015B3BEA, 0x01596596, 0x01579436, 0x0155C7B5,
    0x01540000, 0x01523D04, 0x01507EAE, 0x014EC4EC, 0x014D0FAC, 0x014B5EDD, 0x0149B26D, 0x01480A4B,
    0x01466666, 0x0144C6B0, 0x01432B17, 0x0141938C, 0x01400000, 0x013E7064, 0x013CE4A9, 0x013B5CC1,
    0x0139D89E, 0x01385831, 0x0136DB6E, 0x01356246, 0x0133ECAE, 0x01327A97, 0x01310BF6, 0x012FA0BF,
    0x012E38E4, 0x012CD45A, 0x012B7315, 0x012A150B, 0x0128BA2F, 0x01276276, 0x01260DD6, 0x0124BC45,
    0x01236DB7, 0x01222222, 0x0120D97D, 0x011F93BC, 0x011E50D8, 0x011D10C5, 0x011BD37A, 0x011A98EF,
    0x0119611A, 0x01182BF3, 0x0116F970, 0x0115C988, 0x01149C34, 0x0113716B, 0x01124925, 0x01112359,
    0x01100000, 0x010EDF12, 0x010DC087, 0x010CA458, 0x010B8A7E, 0x010A72F0, 0x01095DA9, 0x01084AA0,
    0x010739CE, 0x01062B2E, 0x01051EB8, 0x01041466, 0x01030C31, 0x01020612, 0x01010204, 0x01000000,
};

// ============================================================================
// SkColorTable.cpp
// ============================================================================

sk_sp<SkColorTable> SkColorTable::Make(const uint8_t tableA[256],
                                       const uint8_t tableR[256],
                                       const uint8_t tableG[256],
                                       const uint8_t tableB[256]) {
    if (!tableA && !tableR && !tableG && !tableB) {
        return nullptr;
    }

    SkBitmap table;
    if (!table.tryAllocPixels(SkImageInfo::MakeA8(256, 4))) {
        return nullptr;
    }
    if (!skia_rs_color_table_fill(tableA, tableR, tableG, tableB, table.getAddr8(0, 0))) {
        return nullptr;
    }
    table.setImmutable();
    return sk_sp<SkColorTable>(new SkColorTable(table));
}

void SkColorTable::flatten(SkWriteBuffer& buffer) const {
    buffer.writeByteArray(fTable.getAddr8(0, 0), 4 * 256);
}

sk_sp<SkColorTable> SkColorTable::Deserialize(SkReadBuffer& buffer) {
    uint8_t argb[4 * 256];
    if (buffer.readByteArray(argb, sizeof(argb))) {
        return SkColorTable::Make(argb + 0 * 256, argb + 1 * 256, argb + 2 * 256, argb + 3 * 256);
    }
    return nullptr;
}

// ============================================================================
// SkMasks.cpp
// ============================================================================

uint8_t SkMasks::getRed(uint32_t pixel) const {
    return skia_rs_masks_get_red(this, pixel);
}

uint8_t SkMasks::getGreen(uint32_t pixel) const {
    return skia_rs_masks_get_green(this, pixel);
}

uint8_t SkMasks::getBlue(uint32_t pixel) const {
    return skia_rs_masks_get_blue(this, pixel);
}

uint8_t SkMasks::getAlpha(uint32_t pixel) const {
    return skia_rs_masks_get_alpha(this, pixel);
}

SkMasks* SkMasks::CreateMasks(InputMasks masks, int bytesPerPixel) {
    SkASSERT(0 < bytesPerPixel && bytesPerPixel <= 4);
    struct RawMasks {
        MaskInfo red, green, blue, alpha;
    };
    static_assert(sizeof(RawMasks) == sizeof(SkMasks));
    RawMasks out;
    if (!skia_rs_masks_create(masks, bytesPerPixel, reinterpret_cast<SkMasks*>(&out))) {
        return nullptr;
    }
    return new SkMasks(out.red, out.green, out.blue, out.alpha);
}

// ============================================================================
// SkImageInfo.cpp
// ============================================================================

int SkColorTypeBytesPerPixel(SkColorType ct) {
    return skia_rs_color_type_bytes_per_pixel(static_cast<int>(ct));
}

bool SkColorTypeIsAlwaysOpaque(SkColorType ct) {
    return skia_rs_color_type_is_always_opaque(static_cast<int>(ct));
}

SkColorInfo::SkColorInfo() = default;
SkColorInfo::~SkColorInfo() = default;

SkColorInfo::SkColorInfo(SkColorType ct, SkAlphaType at, sk_sp<SkColorSpace> cs)
        : fColorSpace(std::move(cs)), fColorType(ct), fAlphaType(at) {}

SkColorInfo::SkColorInfo(const SkColorInfo&) = default;
SkColorInfo::SkColorInfo(SkColorInfo&&) = default;

SkColorInfo& SkColorInfo::operator=(const SkColorInfo&) = default;
SkColorInfo& SkColorInfo::operator=(SkColorInfo&&) = default;

SkColorSpace* SkColorInfo::colorSpace() const { return fColorSpace.get(); }
sk_sp<SkColorSpace> SkColorInfo::refColorSpace() const { return fColorSpace; }

bool SkColorInfo::operator==(const SkColorInfo& other) const {
    return fColorType == other.fColorType && fAlphaType == other.fAlphaType &&
           SkColorSpace::Equals(fColorSpace.get(), other.fColorSpace.get());
}

bool SkColorInfo::operator!=(const SkColorInfo& other) const { return !(*this == other); }

SkColorInfo SkColorInfo::makeAlphaType(SkAlphaType newAlphaType) const {
    return SkColorInfo(this->colorType(), newAlphaType, this->refColorSpace());
}

SkColorInfo SkColorInfo::makeColorType(SkColorType newColorType) const {
    return SkColorInfo(newColorType, this->alphaType(), this->refColorSpace());
}

SkColorInfo SkColorInfo::makeColorSpace(sk_sp<SkColorSpace> cs) const {
    return SkColorInfo(this->colorType(), this->alphaType(), std::move(cs));
}

int SkColorInfo::bytesPerPixel() const { return SkColorTypeBytesPerPixel(fColorType); }

bool SkColorInfo::gammaCloseToSRGB() const {
    return fColorSpace && fColorSpace->gammaCloseToSRGB();
}

int SkColorInfo::shiftPerPixel() const { return SkColorTypeShiftPerPixel(fColorType); }

size_t SkImageInfo::computeOffset(int x, int y, size_t rowBytes) const {
    SkASSERTF(x >= 0 && x < this->width(), "x=%d; width=%d\n", x, this->width());
    SkASSERTF(y >= 0 && y < this->height(), "y=%d; height=%d\n", y, this->height());
    return skia_rs_color_type_compute_offset(static_cast<int>(this->colorType()), x, y, rowBytes);
}

size_t SkImageInfo::computeByteSize(size_t rowBytes) const {
    return skia_rs_image_info_compute_byte_size(
            this->width(), this->height(), static_cast<int>(this->colorType()), rowBytes);
}

SkColorSpace* SkImageInfo::colorSpace() const { return fColorInfo.colorSpace(); }
sk_sp<SkColorSpace> SkImageInfo::refColorSpace() const { return fColorInfo.refColorSpace(); }

SkImageInfo SkImageInfo::makeColorSpace(sk_sp<SkColorSpace> cs) const {
    return Make(fDimensions, fColorInfo.makeColorSpace(std::move(cs)));
}

SkImageInfo SkImageInfo::Make(int width, int height, SkColorType ct, SkAlphaType at) {
    return Make(width, height, ct, at, nullptr);
}

SkImageInfo SkImageInfo::Make(int width, int height, SkColorType ct, SkAlphaType at,
                              sk_sp<SkColorSpace> cs) {
    return SkImageInfo({width, height}, {ct, at, std::move(cs)});
}

SkImageInfo SkImageInfo::Make(SkISize dimensions, SkColorType ct, SkAlphaType at) {
    return Make(dimensions, ct, at, nullptr);
}

SkImageInfo SkImageInfo::Make(SkISize dimensions, SkColorType ct, SkAlphaType at,
                              sk_sp<SkColorSpace> cs) {
    return SkImageInfo(dimensions, {ct, at, std::move(cs)});
}

SkImageInfo SkImageInfo::MakeN32(int width, int height, SkAlphaType at) {
    return MakeN32(width, height, at, nullptr);
}

SkImageInfo SkImageInfo::MakeN32(int width, int height, SkAlphaType at, sk_sp<SkColorSpace> cs) {
    return Make({width, height}, kN32_SkColorType, at, std::move(cs));
}

SkImageInfo SkImageInfo::MakeS32(int width, int height, SkAlphaType at) {
    return SkImageInfo({width, height}, {kN32_SkColorType, at, SkColorSpace::MakeSRGB()});
}

SkImageInfo SkImageInfo::MakeN32Premul(int width, int height) {
    return MakeN32Premul(width, height, nullptr);
}

SkImageInfo SkImageInfo::MakeN32Premul(int width, int height, sk_sp<SkColorSpace> cs) {
    return Make({width, height}, kN32_SkColorType, kPremul_SkAlphaType, std::move(cs));
}

SkImageInfo SkImageInfo::MakeN32Premul(SkISize dimensions) {
    return MakeN32Premul(dimensions, nullptr);
}

SkImageInfo SkImageInfo::MakeN32Premul(SkISize dimensions, sk_sp<SkColorSpace> cs) {
    return Make(dimensions, kN32_SkColorType, kPremul_SkAlphaType, std::move(cs));
}

SkImageInfo SkImageInfo::MakeA8(int width, int height) {
    return Make({width, height}, kAlpha_8_SkColorType, kPremul_SkAlphaType, nullptr);
}

SkImageInfo SkImageInfo::MakeA8(SkISize dimensions) {
    return Make(dimensions, kAlpha_8_SkColorType, kPremul_SkAlphaType, nullptr);
}

SkImageInfo SkImageInfo::MakeUnknown(int width, int height) {
    return Make({width, height}, kUnknown_SkColorType, kUnknown_SkAlphaType, nullptr);
}

#ifdef SK_DEBUG
void SkImageInfo::validate() const {
    SkASSERT(fDimensions.width() >= 0);
    SkASSERT(fDimensions.height() >= 0);
    SkASSERT(SkColorTypeIsValid(this->colorType()));
    SkASSERT(SkAlphaTypeIsValid(this->alphaType()));
}
#endif

bool SkColorTypeValidateAlphaType(SkColorType colorType, SkAlphaType alphaType,
                                  SkAlphaType* canonical) {
    int canon = 0;
    if (!skia_rs_color_type_validate_alpha_type(static_cast<int>(colorType),
                                                static_cast<int>(alphaType), &canon)) {
        return false;
    }
    if (canonical) {
        *canonical = static_cast<SkAlphaType>(canon);
    }
    return true;
}

// ============================================================================
// SkBlendMode.cpp
// ============================================================================

bool SkBlendMode_ShouldPreScaleCoverage(SkBlendMode mode, bool rgb_coverage) {
    return skia_rs_blend_mode_should_pre_scale_coverage(static_cast<int>(mode), rgb_coverage);
}

bool SkBlendMode_SupportsCoverageAsAlpha(SkBlendMode mode) {
    return skia_rs_blend_mode_supports_coverage_as_alpha(static_cast<int>(mode));
}

bool SkBlendMode_AsCoeff(SkBlendMode mode, SkBlendModeCoeff* src, SkBlendModeCoeff* dst) {
    int s = 0, d = 0;
    if (!skia_rs_blend_mode_as_coeff(static_cast<int>(mode), &s, &d)) {
        return false;
    }
    if (src) {
        *src = static_cast<SkBlendModeCoeff>(s);
    }
    if (dst) {
        *dst = static_cast<SkBlendModeCoeff>(d);
    }
    return true;
}

void SkBlendMode_AppendStages(SkBlendMode mode, SkRasterPipeline* p) {
    switch (mode) {
        case SkBlendMode::kClear:      p->append(SkRasterPipelineOp::clear); return;
        case SkBlendMode::kSrc:        return;
        case SkBlendMode::kDst:        p->append(SkRasterPipelineOp::move_dst_src); return;
        case SkBlendMode::kSrcOver:    p->append(SkRasterPipelineOp::srcover); return;
        case SkBlendMode::kDstOver:    p->append(SkRasterPipelineOp::dstover); return;
        case SkBlendMode::kSrcIn:      p->append(SkRasterPipelineOp::srcin); return;
        case SkBlendMode::kDstIn:      p->append(SkRasterPipelineOp::dstin); return;
        case SkBlendMode::kSrcOut:     p->append(SkRasterPipelineOp::srcout); return;
        case SkBlendMode::kDstOut:     p->append(SkRasterPipelineOp::dstout); return;
        case SkBlendMode::kSrcATop:    p->append(SkRasterPipelineOp::srcatop); return;
        case SkBlendMode::kDstATop:    p->append(SkRasterPipelineOp::dstatop); return;
        case SkBlendMode::kXor:        p->append(SkRasterPipelineOp::xor_); return;
        case SkBlendMode::kPlus:       p->append(SkRasterPipelineOp::plus_); return;
        case SkBlendMode::kModulate:   p->append(SkRasterPipelineOp::modulate); return;
        case SkBlendMode::kScreen:     p->append(SkRasterPipelineOp::screen); return;
        case SkBlendMode::kOverlay:    p->append(SkRasterPipelineOp::overlay); return;
        case SkBlendMode::kDarken:     p->append(SkRasterPipelineOp::darken); return;
        case SkBlendMode::kLighten:    p->append(SkRasterPipelineOp::lighten); return;
        case SkBlendMode::kColorDodge: p->append(SkRasterPipelineOp::colordodge); return;
        case SkBlendMode::kColorBurn:  p->append(SkRasterPipelineOp::colorburn); return;
        case SkBlendMode::kHardLight:  p->append(SkRasterPipelineOp::hardlight); return;
        case SkBlendMode::kSoftLight:  p->append(SkRasterPipelineOp::softlight); return;
        case SkBlendMode::kDifference: p->append(SkRasterPipelineOp::difference); return;
        case SkBlendMode::kExclusion:  p->append(SkRasterPipelineOp::exclusion); return;
        case SkBlendMode::kMultiply:   p->append(SkRasterPipelineOp::multiply); return;
        case SkBlendMode::kHue:        p->append(SkRasterPipelineOp::hue); return;
        case SkBlendMode::kSaturation: p->append(SkRasterPipelineOp::saturation); return;
        case SkBlendMode::kColor:      p->append(SkRasterPipelineOp::color); return;
        case SkBlendMode::kLuminosity: p->append(SkRasterPipelineOp::luminosity); return;
        default:                       p->append(SkRasterPipelineOp::srcover); return;
    }
    SkUNREACHABLE;
}

SkPMColor4f SkBlendMode_Apply(SkBlendMode mode, const SkPMColor4f& src, const SkPMColor4f& dst) {
    SkPMColor4f fast_res;
    if (skia_rs_blend_mode_apply_fast(static_cast<int>(mode), &src, &dst, &fast_res)) {
        return fast_res;
    }

    SkRasterPipeline_<256> p;
    SkPMColor4f src_storage = src;
    SkPMColor4f dst_storage = dst;
    SkPMColor4f res_storage;
    SkRasterPipelineContexts::MemoryCtx src_ctx = {&src_storage, 0};
    SkRasterPipelineContexts::MemoryCtx dst_ctx = {&dst_storage, 0};
    SkRasterPipelineContexts::MemoryCtx res_ctx = {&res_storage, 0};

    p.append(SkRasterPipelineOp::load_f32, &dst_ctx);
    p.append(SkRasterPipelineOp::move_src_dst);
    p.append(SkRasterPipelineOp::load_f32, &src_ctx);
    SkBlendMode_AppendStages(mode, &p);
    p.append(SkRasterPipelineOp::store_f32, &res_ctx);
    p.run(0, 0, 1, 1);
    return res_storage;
}

const char* SkBlendMode_Name(SkBlendMode bm) {
    return skia_rs_blend_mode_name(static_cast<int>(bm));
}

SkBlendFastPath CheckFastPath(const SkPaint& paint, bool dstIsOpaque) {
    const auto bm = paint.asBlendMode();
    const bool justSolidColor =
            (SK_AlphaOPAQUE == paint.getAlpha() && !paint.getColorFilter() && !paint.getShader());
    int res = skia_rs_blend_mode_check_fast_path(
            bm.has_value(),
            bm.has_value() ? static_cast<int>(bm.value()) : 0,
            justSolidColor,
            dstIsOpaque);
    return static_cast<SkBlendFastPath>(res);
}

// ============================================================================
// SkBlendModeBlender.cpp
// ============================================================================

const SkBlender* GetBlendModeSingleton(SkBlendMode mode) {
#define RETURN_SINGLETON_BLENDER(m)                            \
    case m: {                                                  \
        static SkNoDestructor<SkBlendModeBlender> sBlender(m); \
        return sBlender.get();                                 \
    }

    switch (mode) {
        RETURN_SINGLETON_BLENDER(SkBlendMode::kClear)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kSrc)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kDst)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kSrcOver)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kDstOver)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kSrcIn)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kDstIn)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kSrcOut)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kDstOut)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kSrcATop)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kDstATop)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kXor)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kPlus)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kModulate)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kScreen)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kOverlay)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kDarken)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kLighten)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kColorDodge)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kColorBurn)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kHardLight)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kSoftLight)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kDifference)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kExclusion)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kMultiply)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kHue)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kSaturation)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kColor)
        RETURN_SINGLETON_BLENDER(SkBlendMode::kLuminosity)
    }

    SkDEBUGFAILF("invalid blend mode %d", (int)mode);
    return nullptr;

#undef RETURN_SINGLETON_BLENDER
}

sk_sp<SkBlender> SkBlender::Mode(SkBlendMode mode) {
    return sk_ref_sp(GetBlendModeSingleton(mode));
}

sk_sp<SkFlattenable> SkBlendModeBlender::CreateProc(SkReadBuffer& buffer) {
    SkBlendMode mode = buffer.read32LE(SkBlendMode::kLastMode);
    return SkBlender::Mode(mode);
}

void SkBlendModeBlender::flatten(SkWriteBuffer& buffer) const {
    buffer.writeInt((int)fMode);
}

bool SkBlendModeBlender::onAppendStages(const SkStageRec& rec) const {
    SkBlendMode_AppendStages(fMode, rec.fPipeline);
    return true;
}

bool SkBlenderBase::affectsTransparentBlack() const {
    auto bm = this->asBlendMode();
    return skia_rs_blend_mode_affects_transparent_black(
            bm.has_value(), bm.has_value() ? static_cast<int>(bm.value()) : 0);
}
