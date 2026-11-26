/*
 * Safe Rust bridge for Layer 2.4 skia-rs-core-types:
 *   SkFlattenable.cpp, SkPaint.cpp, SkPaintPriv.cpp, SkDescriptor.cpp.
 */

#include "include/core/SkAlphaType.h"
#include "include/core/SkBlendMode.h"
#include "include/core/SkBlender.h"
#include "include/core/SkColorFilter.h"
#include "include/core/SkColorType.h"
#include "include/core/SkData.h"
#include "include/core/SkFlattenable.h"
#include "include/core/SkImageFilter.h"
#include "include/core/SkMaskFilter.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPathEffect.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkScalar.h"
#include "include/core/SkSerialProcs.h"
#include "include/core/SkShader.h"
#include "include/core/SkString.h"
#include "include/core/SkStrokeRec.h"
#include "include/core/SkTypes.h"
#include "include/private/SkAlign.h"
#include "include/private/SkAssert.h"
#include "include/private/SkDebug.h"
#include "include/private/SkTDArray.h"
#include "include/private/SkTPin.h"
#include "include/private/SkTemplates.h"
#include "include/private/SkTo.h"
#include "src/core/SkBlenderBase.h"
#include "src/core/SkChecksum.h"
#include "src/core/SkColorSpacePriv.h"
#include "src/core/SkColorSpaceXformSteps.h"
#include "src/core/SkDescriptor.h"
#include "src/core/SkMaskFilterBase.h"
#include "src/core/SkPaintDefaults.h"
#include "src/core/SkPaintPriv.h"
#include "src/core/SkPathEffectBase.h"
#include "src/core/SkPicturePriv.h"
#include "src/core/SkPtrRecorder.h"
#include "src/core/SkReadBuffer.h"
#include "src/core/SkSafeRange.h"
#include "src/core/SkSharedMutex.h"
#include "src/core/SkWriteBuffer.h"
#include "src/effects/colorfilters/SkColorFilterBase.h"
#include "src/shaders/SkColorFilterShader.h"
#include "src/shaders/SkShaderBase.h"

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <memory>
#include <new>
#include <optional>
#include <utility>

class SkColorSpace;

extern "C" {

// SkFlattenable
void skia_rs_flattenable_register(const uint8_t* name, size_t name_len, uintptr_t factory_token);
void skia_rs_flattenable_finalize();
uintptr_t skia_rs_flattenable_name_to_factory(const uint8_t* name, size_t name_len);
uintptr_t skia_rs_flattenable_factory_to_name(uintptr_t factory_token);

// SkPaint & SkPaintPriv
struct SkiaRsPaintUnpackedV68 {
    bool    anti_alias;
    bool    dither;
    bool    has_blend_mode;
    uint8_t blend_mode;
    uint8_t stroke_cap;
    uint8_t stroke_join;
    uint8_t style;
    uint8_t flat_flags;
    bool    safe_ok;
};

float skia_rs_paint_pin_alphaf(float a);
bool skia_rs_paint_nothing_to_draw(bool has_blend_mode, uint8_t blend_mode, uint8_t alpha,
                                   bool cf_affects_alpha, bool imf_affects_alpha);
bool skia_rs_paint_overwrites(bool has_paint, uint8_t override_opacity, bool changes_alpha,
                              uint8_t paint_alpha, bool has_shader, bool shader_is_opaque,
                              bool has_blend_mode, uint8_t blend_mode);
bool skia_rs_paint_should_dither(bool is_dither, int32_t dst_color_type,
                                 bool has_image_filter, bool has_mask_filter,
                                 bool has_shader, bool shader_is_constant);
uint32_t skia_rs_paint_pack_v68(bool anti_alias, bool dither,
                                bool has_blend_mode, uint8_t blend_mode,
                                uint8_t stroke_cap, uint8_t stroke_join,
                                uint8_t style, uint8_t flat_flags);
SkiaRsPaintUnpackedV68 skia_rs_paint_unpack_v68(uint32_t packed);
void skia_rs_paint_outset_rect(const SkRect* src, float radius, SkRect* out);

// SkDescriptor
uint32_t skia_rs_descriptor_compute_checksum(const uint8_t* desc_bytes, size_t desc_len);
size_t skia_rs_descriptor_add_entry(uint8_t* buf, size_t buf_cap, uint32_t tag,
                                    size_t payload_len, const uint8_t* data);
bool skia_rs_descriptor_find_entry(const uint8_t* buf, size_t buf_len, uint32_t tag,
                                   size_t* out_data_offset, uint32_t* out_data_len);
bool skia_rs_descriptor_is_valid(const uint8_t* buf, size_t buf_len,
                                 uint32_t rec_tag, size_t rec_size);
bool skia_rs_descriptor_equals(const uint8_t* a, size_t a_len, const uint8_t* b, size_t b_len);
bool skia_rs_descriptor_validate_header_for_read(uint32_t header_length, size_t available,
                                                 uint32_t* out_body_len);

}  // extern "C"

static_assert(sizeof(SkRect) == 16, "SkRect layout mismatch");
static_assert(sizeof(SkDescriptor) == 12, "SkDescriptor header layout mismatch");
static_assert(sizeof(SkiaRsPaintUnpackedV68) == 9, "PaintUnpackedV68 layout mismatch");

// ============================================================================
// SkFlattenable, SkNamedFactorySet, SkRefCntSet
// ============================================================================

SkNamedFactorySet::SkNamedFactorySet() : fNextAddedFactory(0) {}

uint32_t SkNamedFactorySet::find(SkFlattenable::Factory factory) {
    uint32_t index = fFactorySet.find(factory);
    if (index > 0) {
        return index;
    }
    const char* name = SkFlattenable::FactoryToName(factory);
    if (nullptr == name) {
        return 0;
    }
    *fNames.append() = name;
    return fFactorySet.add(factory);
}

const char* SkNamedFactorySet::getNextAddedFactoryName() {
    if (fNextAddedFactory < fNames.size()) {
        return fNames[fNextAddedFactory++];
    }
    return nullptr;
}

SkRefCntSet::~SkRefCntSet() {
    this->reset();
}

void SkRefCntSet::incPtr(void* ptr) {
    ((SkRefCnt*)ptr)->ref();
}

void SkRefCntSet::decPtr(void* ptr) {
    ((SkRefCnt*)ptr)->unref();
}

void SkFlattenable::Finalize() {
    skia_rs_flattenable_finalize();
}

void SkFlattenable::Register(const char name[], Factory factory) {
    SkASSERT(name);
    SkASSERT(factory);
    skia_rs_flattenable_register(reinterpret_cast<const uint8_t*>(name),
                                 strlen(name),
                                 reinterpret_cast<uintptr_t>(factory));
}

SkFlattenable::Factory SkFlattenable::NameToFactory(const char name[]) {
    RegisterFlattenablesIfNeeded();
    if (!name) {
        return nullptr;
    }
    uintptr_t token = skia_rs_flattenable_name_to_factory(
            reinterpret_cast<const uint8_t*>(name), strlen(name));
    return reinterpret_cast<Factory>(token);
}

const char* SkFlattenable::FactoryToName(Factory fact) {
    RegisterFlattenablesIfNeeded();
    uintptr_t nameToken = skia_rs_flattenable_factory_to_name(
            reinterpret_cast<uintptr_t>(fact));
    return reinterpret_cast<const char*>(nameToken);
}

sk_sp<SkData> SkFlattenable::serialize(const SkSerialProcs* procs) const {
    SkSerialProcs p;
    if (procs) {
        p = *procs;
    }
    SkBinaryWriteBuffer writer(p);
    writer.writeFlattenable(this);
    size_t size = writer.bytesWritten();
    auto data = SkData::MakeUninitialized(size);
    writer.writeToMemory(data->writable_data());
    return data;
}

size_t SkFlattenable::serialize(void* memory, size_t memory_size,
                                const SkSerialProcs* procs) const {
    SkSerialProcs p;
    if (procs) {
        p = *procs;
    }
    SkBinaryWriteBuffer writer(memory, memory_size, p);
    writer.writeFlattenable(this);
    return writer.usingInitialStorage() ? writer.bytesWritten() : 0u;
}

sk_sp<SkFlattenable> SkFlattenable::Deserialize(SkFlattenable::Type type, const void* data,
                                                size_t size, const SkDeserialProcs* procs) {
    SkReadBuffer buffer(data, size);
    if (procs) {
        buffer.setDeserialProcs(*procs);
    }
    return sk_sp<SkFlattenable>(buffer.readFlattenable(type));
}

// ============================================================================
// SkPaint
// ============================================================================

SkPaint::SkPaint()
    : fColor4f{0, 0, 0, 1}
    , fWidth{0}
    , fMiterLimit{SkPaintDefaults_MiterLimit}
    , fBitfields{(unsigned)false,
                 (unsigned)false,
                 (unsigned)SkPaint::kDefault_Cap,
                 (unsigned)SkPaint::kDefault_Join,
                 (unsigned)SkPaint::kFill_Style,
                 0}
{
    static_assert(sizeof(fBitfields) == sizeof(fBitfieldsUInt), "");
}

SkPaint::SkPaint(const SkColor4f& color, SkColorSpace* colorSpace) : SkPaint() {
    this->setColor(color, colorSpace);
}

SkPaint::SkPaint(const SkPaint& src) = default;
SkPaint::SkPaint(SkPaint&& src) = default;
SkPaint::~SkPaint() = default;
SkPaint& SkPaint::operator=(const SkPaint& src) = default;
SkPaint& SkPaint::operator=(SkPaint&& src) = default;

bool operator==(const SkPaint& a, const SkPaint& b) {
#define EQUAL(field) (a.field == b.field)
    return EQUAL(fPathEffect)
        && EQUAL(fShader)
        && EQUAL(fMaskFilter)
        && EQUAL(fColorFilter)
        && EQUAL(fBlender)
        && EQUAL(fImageFilter)
        && EQUAL(fColor4f)
        && EQUAL(fWidth)
        && EQUAL(fMiterLimit)
        && EQUAL(fBitfieldsUInt);
#undef EQUAL
}

#define DEFINE_FIELD_REF(type) \
    sk_sp<Sk##type> SkPaint::ref##type() const { return f##type; }
DEFINE_FIELD_REF(ColorFilter)
DEFINE_FIELD_REF(Blender)
DEFINE_FIELD_REF(ImageFilter)
DEFINE_FIELD_REF(MaskFilter)
DEFINE_FIELD_REF(PathEffect)
DEFINE_FIELD_REF(Shader)
#undef DEFINE_FIELD_REF

#define DEFINE_FIELD_SET(Field) \
    void SkPaint::set##Field(sk_sp<Sk##Field> f) { f##Field = std::move(f); }
DEFINE_FIELD_SET(ColorFilter)
DEFINE_FIELD_SET(ImageFilter)
DEFINE_FIELD_SET(MaskFilter)
DEFINE_FIELD_SET(PathEffect)
DEFINE_FIELD_SET(Shader)
#undef DEFINE_FIELD_SET

void SkPaint::reset() { *this = SkPaint(); }

void SkPaint::setStyle(Style style) {
    if ((unsigned)style < kStyleCount) {
        fBitfields.fStyle = style;
    }
}

void SkPaint::setStroke(bool isStroke) {
    fBitfields.fStyle = isStroke ? kStroke_Style : kFill_Style;
}

void SkPaint::setColor(SkColor color) {
    fColor4f = SkColor4f::FromColor(color);
}

void SkPaint::setColor(const SkColor4f& color, SkColorSpace* colorSpace) {
    SkColorSpaceXformSteps steps{colorSpace,          kUnpremul_SkAlphaType,
                                 sk_srgb_singleton(), kUnpremul_SkAlphaType};
    fColor4f = color.pinAlpha();
    steps.apply(fColor4f.vec());
}

void SkPaint::setAlphaf(float a) {
    fColor4f.fA = skia_rs_paint_pin_alphaf(a);
}

void SkPaint::setARGB(U8CPU a, U8CPU r, U8CPU g, U8CPU b) {
    this->setColor(SkColorSetARGB(a, r, g, b));
}

std::optional<SkBlendMode> SkPaint::asBlendMode() const {
    return fBlender ? as_BB(fBlender)->asBlendMode()
                    : SkBlendMode::kSrcOver;
}

SkBlendMode SkPaint::getBlendMode_or(SkBlendMode defaultMode) const {
    return this->asBlendMode().value_or(defaultMode);
}

bool SkPaint::isSrcOver() const {
    return !fBlender || as_BB(fBlender)->asBlendMode() == SkBlendMode::kSrcOver;
}

void SkPaint::setBlendMode(SkBlendMode mode) {
    this->setBlender(mode == SkBlendMode::kSrcOver ? nullptr : SkBlender::Mode(mode));
}

void SkPaint::setBlender(sk_sp<SkBlender> blender) {
    fBlender = std::move(blender);
}

void SkPaint::setStrokeWidth(SkScalar width) {
    if (width >= 0) {
        fWidth = width;
    }
}

void SkPaint::setStrokeMiter(SkScalar limit) {
    if (limit >= 0) {
        fMiterLimit = limit;
    }
}

void SkPaint::setStrokeCap(Cap ct) {
    if ((unsigned)ct < kCapCount) {
        fBitfields.fCapType = SkToU8(ct);
    }
}

void SkPaint::setStrokeJoin(Join jt) {
    if ((unsigned)jt < kJoinCount) {
        fBitfields.fJoinType = SkToU8(jt);
    }
}

bool SkPaint::canComputeFastBounds() const {
    if (this->getImageFilter() && !this->getImageFilter()->canComputeFastBounds()) {
        return false;
    }
    if (this->getPathEffect() &&
        !as_PEB(this->getPathEffect())->computeFastBounds(nullptr)) {
        return false;
    }
    return true;
}

const SkRect& SkPaint::computeFastBounds(const SkRect& orig, SkRect* storage) const {
    SkASSERT(orig.isSorted());
    SkPaint::Style style = this->getStyle();
    if (kFill_Style == style) {
        uintptr_t effects = 0;
        effects |= reinterpret_cast<uintptr_t>(this->getMaskFilter());
        effects |= reinterpret_cast<uintptr_t>(this->getPathEffect());
        effects |= reinterpret_cast<uintptr_t>(this->getImageFilter());
        if (!effects) {
            return orig;
        }
    }
    return this->doComputeFastBounds(orig, storage, style);
}

const SkRect& SkPaint::doComputeFastBounds(const SkRect& origSrc,
                                           SkRect* storage,
                                           Style style) const {
    SkASSERT(storage);

    const SkRect* src = &origSrc;

    SkRect tmpSrc;
    if (this->getPathEffect()) {
        tmpSrc = origSrc;
        SkAssertResult(as_PEB(this->getPathEffect())->computeFastBounds(&tmpSrc));
        src = &tmpSrc;
    }

    SkScalar radius = SkStrokeRec::GetInflationRadius(*this, style);
    skia_rs_paint_outset_rect(src, radius, storage);

    if (this->getMaskFilter()) {
        as_MFB(this->getMaskFilter())->computeFastBounds(*storage, storage);
    }

    if (this->getImageFilter()) {
        *storage = this->getImageFilter()->computeFastBounds(*storage);
    }

    return *storage;
}

static bool affects_alpha(const SkColorFilter* cf) {
    return cf && !as_CFB(cf)->isAlphaUnchanged();
}

static bool affects_alpha(const SkImageFilter* imf) {
    return imf != nullptr;
}

bool SkPaint::nothingToDraw() const {
    auto bm = this->asBlendMode();
    return skia_rs_paint_nothing_to_draw(
            bm.has_value(),
            bm.has_value() ? static_cast<uint8_t>(bm.value()) : 0,
            this->getAlpha(),
            affects_alpha(fColorFilter.get()),
            affects_alpha(fImageFilter.get()));
}

// ============================================================================
// SkPaintPriv
// ============================================================================

static bool changes_alpha(const SkPaint& paint) {
    SkColorFilter* cf = paint.getColorFilter();
    return cf && !as_CFB(cf)->isAlphaUnchanged();
}

bool SkPaintPriv::Overwrites(const SkPaint* paint, ShaderOverrideOpacity overrideOpacity) {
    if (!paint) {
        return skia_rs_paint_overwrites(
                false, static_cast<uint8_t>(overrideOpacity), false, 0, false, false, false, 0);
    }
    const bool hasShader = paint->getShader() != nullptr;
    const bool shaderOpaque = hasShader && paint->getShader()->isOpaque();
    const auto bm = paint->asBlendMode();
    return skia_rs_paint_overwrites(
            true,
            static_cast<uint8_t>(overrideOpacity),
            changes_alpha(*paint),
            paint->getAlpha(),
            hasShader,
            shaderOpaque,
            bm.has_value(),
            bm.has_value() ? static_cast<uint8_t>(bm.value()) : 0);
}

bool SkPaintPriv::ShouldDither(const SkPaint& p, SkColorType dstCT) {
    const bool hasShader = p.getShader() != nullptr;
    const bool shaderIsConstant = hasShader && as_SB(p.getShader())->isConstant();
    return skia_rs_paint_should_dither(
            p.isDither(),
            static_cast<int32_t>(dstCT),
            p.getImageFilter() != nullptr,
            p.getMaskFilter() != nullptr,
            hasShader,
            shaderIsConstant);
}

static bool just_a_color(const SkPaint& paint, SkColor4f* color) {
    SkColor4f c = paint.getColor4f();

    const auto* shader = as_SB(paint.getShader());
    if (shader && !shader->asLuminanceColor(&c)) {
        return false;
    }
    if (paint.getColorFilter()) {
        SkColorSpace* cs = nullptr;
        c = paint.getColorFilter()->filterColor4f(c, cs, cs);
    }
    if (color) {
        *color = c;
    }
    return true;
}

SkColor SkPaintPriv::ComputeLuminanceColor(const SkPaint& paint) {
    SkColor4f c;
    if (!just_a_color(paint, &c)) {
        c = {0.5f, 0.5f, 0.5f, 1.0f};
    }
    return c.toSkColor();
}

void SkPaintPriv::RemoveColorFilter(SkPaint* p, SkColorSpace* dstCS) {
    if (SkColorFilter* filter = p->getColorFilter()) {
        if (SkShader* shader = p->getShader()) {
            p->setShader(SkColorFilterShader::Make(sk_ref_sp(shader),
                                                   p->getAlphaf(),
                                                   sk_ref_sp(filter)));
            p->setAlphaf(1.0f);
        } else {
            p->setColor(filter->filterColor4f(p->getColor4f(), sk_srgb_singleton(), dstCS), dstCS);
        }
        p->setColorFilter(nullptr);
    }
}

enum FlatFlags {
    kHasTypeface_FlatFlag = 0x1,
    kHasEffects_FlatFlag  = 0x2,
};

void SkPaintPriv::Flatten(const SkPaint& paint, SkWriteBuffer& buffer) {
    uint8_t flatFlags = 0;

    if (paint.getPathEffect() ||
        paint.getShader() ||
        paint.getMaskFilter() ||
        paint.getColorFilter() ||
        paint.getImageFilter() ||
        !paint.asBlendMode()) {
        flatFlags |= kHasEffects_FlatFlag;
    }

    buffer.writeScalar(paint.getStrokeWidth());
    buffer.writeScalar(paint.getStrokeMiter());
    buffer.writeColor4f(paint.getColor4f());

    const auto bm = paint.asBlendMode();
    uint32_t packed = skia_rs_paint_pack_v68(
            paint.isAntiAlias(),
            paint.isDither(),
            bm.has_value(),
            bm.has_value() ? static_cast<uint8_t>(bm.value()) : 0,
            static_cast<uint8_t>(paint.getStrokeCap()),
            static_cast<uint8_t>(paint.getStrokeJoin()),
            static_cast<uint8_t>(paint.getStyle()),
            flatFlags);
    buffer.write32(packed);

    if (flatFlags & kHasEffects_FlatFlag) {
        buffer.writeFlattenable(paint.getPathEffect());
        buffer.writeFlattenable(paint.getShader());
        buffer.writeFlattenable(paint.getMaskFilter());
        buffer.writeFlattenable(paint.getColorFilter());
        buffer.writeFlattenable(paint.getImageFilter());
        buffer.writeFlattenable(paint.getBlender());
    }
}

SkPaint SkPaintPriv::Unflatten(SkReadBuffer& buffer) {
    SkPaint paint;

    paint.setStrokeWidth(buffer.readScalar());
    paint.setStrokeMiter(buffer.readScalar());
    {
        SkColor4f color;
        buffer.readColor4f(&color);
        paint.setColor(color, sk_srgb_singleton());
    }

    SkiaRsPaintUnpackedV68 unpacked = skia_rs_paint_unpack_v68(buffer.readUInt());
    paint.setAntiAlias(unpacked.anti_alias);
    paint.setDither(unpacked.dither);
    if (unpacked.has_blend_mode) {
        paint.setBlendMode(static_cast<SkBlendMode>(unpacked.blend_mode));
    }
    paint.setStrokeCap(static_cast<SkPaint::Cap>(unpacked.stroke_cap));
    paint.setStrokeJoin(static_cast<SkPaint::Join>(unpacked.stroke_join));
    paint.setStyle(static_cast<SkPaint::Style>(unpacked.style));

    unsigned flatFlags = unpacked.flat_flags;

    if (!(flatFlags & kHasEffects_FlatFlag)) {
        paint.setPathEffect(nullptr);
        paint.setShader(nullptr);
        paint.setMaskFilter(nullptr);
        paint.setColorFilter(nullptr);
        paint.setImageFilter(nullptr);
    } else if (buffer.isVersionLT(SkPicturePriv::kSkBlenderInSkPaint)) {
        paint.setPathEffect(buffer.readPathEffect());
        paint.setShader(buffer.readShader());
        paint.setMaskFilter(buffer.readMaskFilter());
        paint.setColorFilter(buffer.readColorFilter());
        (void)buffer.read32();
        paint.setImageFilter(buffer.readImageFilter());
    } else {
        paint.setPathEffect(buffer.readPathEffect());
        paint.setShader(buffer.readShader());
        paint.setMaskFilter(buffer.readMaskFilter());
        paint.setColorFilter(buffer.readColorFilter());
        paint.setImageFilter(buffer.readImageFilter());
        paint.setBlender(buffer.readBlender());
    }

    if (!buffer.validate(unpacked.safe_ok)) {
        paint.reset();
    }
    return paint;
}

// ============================================================================
// SkDescriptor & SkAutoDescriptor
// ============================================================================

std::unique_ptr<SkDescriptor> SkDescriptor::Alloc(size_t length) {
    SkASSERT(length >= sizeof(SkDescriptor) && SkIsAlign4(length));
    void* allocation = ::operator new(length);
    return std::unique_ptr<SkDescriptor>(new (allocation) SkDescriptor{});
}

void SkDescriptor::operator delete(void* p) { ::operator delete(p); }

void* SkDescriptor::operator new(size_t) {
    SK_ABORT("Descriptors are created with placement new.");
}

void SkDescriptor::flatten(SkWriteBuffer& buffer) const {
    buffer.writePad32(static_cast<const void*>(this), this->fLength);
}

void* SkDescriptor::addEntry(uint32_t tag, size_t length, const void* data) {
    static_assert(sizeof(Entry) == 8, "SkDescriptor::Entry layout mismatch");
    SkASSERT(tag);
    SkASSERT(SkIsAlign4(length));
    SkASSERT(this->findEntry(tag, nullptr) == nullptr);

    size_t bufCap = static_cast<size_t>(fLength) + sizeof(Entry) + length;
    size_t dataOffset = skia_rs_descriptor_add_entry(
            reinterpret_cast<uint8_t*>(this),
            bufCap,
            tag,
            length,
            static_cast<const uint8_t*>(data));
    return reinterpret_cast<uint8_t*>(this) + dataOffset;
}

void SkDescriptor::computeChecksum() {
    fChecksum = SkDescriptor::ComputeChecksum(this);
}

const void* SkDescriptor::findEntry(uint32_t tag, uint32_t* length) const {
    size_t dataOffset = 0;
    uint32_t dataLen = 0;
    if (skia_rs_descriptor_find_entry(
            reinterpret_cast<const uint8_t*>(this),
            static_cast<size_t>(fLength),
            tag,
            &dataOffset,
            &dataLen)) {
        if (length) {
            *length = dataLen;
        }
        return reinterpret_cast<const uint8_t*>(this) + dataOffset;
    }
    return nullptr;
}

std::unique_ptr<SkDescriptor> SkDescriptor::copy() const {
    std::unique_ptr<SkDescriptor> desc = SkDescriptor::Alloc(fLength);
    memcpy(desc.get(), this, fLength);
    return desc;
}

bool SkDescriptor::operator==(const SkDescriptor& other) const {
    SkASSERT(SkIsAlign4(fLength));
    return skia_rs_descriptor_equals(
            reinterpret_cast<const uint8_t*>(this), static_cast<size_t>(fLength),
            reinterpret_cast<const uint8_t*>(&other), static_cast<size_t>(other.fLength));
}

SkString SkDescriptor::dumpRec() const {
    const SkScalerContextRec* rec = static_cast<const SkScalerContextRec*>(
            this->findEntry(kRec_SkDescriptorTag, nullptr));

    SkString result;
    result.appendf("    Checksum: %x\n", fChecksum);
    if (rec != nullptr) {
        result.append(rec->dump());
    }
    return result;
}

uint32_t SkDescriptor::ComputeChecksum(const SkDescriptor* desc) {
    return skia_rs_descriptor_compute_checksum(
            reinterpret_cast<const uint8_t*>(desc), static_cast<size_t>(desc->fLength));
}

bool SkDescriptor::isValid() const {
    return skia_rs_descriptor_is_valid(
            reinterpret_cast<const uint8_t*>(this),
            static_cast<size_t>(fLength),
            kRec_SkDescriptorTag,
            sizeof(SkScalerContextRec));
}

SkAutoDescriptor::SkAutoDescriptor() = default;
SkAutoDescriptor::SkAutoDescriptor(size_t size) { this->reset(size); }
SkAutoDescriptor::SkAutoDescriptor(const SkDescriptor& desc) { this->reset(desc); }
SkAutoDescriptor::SkAutoDescriptor(const SkAutoDescriptor& that) {
    this->reset(*that.getDesc());
}
SkAutoDescriptor& SkAutoDescriptor::operator=(const SkAutoDescriptor& that) {
    this->reset(*that.getDesc());
    return *this;
}
SkAutoDescriptor::SkAutoDescriptor(SkAutoDescriptor&& that) {
    if (that.fDesc == (SkDescriptor*)&that.fStorage) {
        this->reset(*that.getDesc());
    } else {
        fDesc = that.fDesc;
        that.fDesc = nullptr;
    }
}
SkAutoDescriptor& SkAutoDescriptor::operator=(SkAutoDescriptor&& that) {
    if (that.fDesc == (SkDescriptor*)&that.fStorage) {
        this->reset(*that.getDesc());
    } else {
        this->free();
        fDesc = that.fDesc;
        that.fDesc = nullptr;
    }
    return *this;
}

SkAutoDescriptor::~SkAutoDescriptor() { this->free(); }

std::optional<SkAutoDescriptor> SkAutoDescriptor::MakeFromBuffer(SkReadBuffer& buffer) {
    SkDescriptor descriptorHeader;
    if (!buffer.readPad32(&descriptorHeader, sizeof(SkDescriptor))) {
        return {};
    }

    uint32_t bodyLength = 0;
    if (!skia_rs_descriptor_validate_header_for_read(
            descriptorHeader.getLength(), buffer.available(), &bodyLength)) {
        return {};
    }

    SkAutoDescriptor ad{descriptorHeader.getLength()};
    memcpy(ad.fDesc, &descriptorHeader, sizeof(SkDescriptor));
    if (!buffer.readPad32(SkTAddOffset<void>(ad.fDesc.get(), sizeof(SkDescriptor)), bodyLength)) {
        return {};
    }

#if defined(SK_BUILD_FOR_FUZZER)
    SkDescriptor::ComputeChecksum(ad.getDesc());
#else
    if (SkDescriptor::ComputeChecksum(ad.getDesc()) != ad.getDesc()->fChecksum) {
        return {};
    }
#endif
    if (!ad.getDesc()->isValid()) {
        return {};
    }

    return {ad};
}

void SkAutoDescriptor::reset(size_t size) {
    this->free();
    if (size <= sizeof(fStorage)) {
        fDesc = new (&fStorage) SkDescriptor{};
    } else {
        fDesc = SkDescriptor::Alloc(size).release();
    }
}

void SkAutoDescriptor::reset(const SkDescriptor& desc) {
    size_t size = desc.getLength();
    this->reset(size);
    memcpy(fDesc, &desc, size);
}

void SkAutoDescriptor::free() {
    if (fDesc == (SkDescriptor*)&fStorage) {
        fDesc->~SkDescriptor();
    } else {
        delete fDesc;
    }
}
