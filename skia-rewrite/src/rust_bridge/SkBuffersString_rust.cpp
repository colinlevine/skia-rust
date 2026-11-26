/*
 * Safe Rust bridge for Layer 2.4 skia-rs-core-types:
 *   SkBuffer.cpp, SkReadBuffer.cpp, SkWriteBuffer.cpp, SkWriter32.cpp,
 *   SkString.cpp, SkStringUtils.cpp.
 */

#include "include/core/SkAlphaType.h"
#include "include/core/SkData.h"
#include "include/core/SkFlattenable.h"
#include "include/core/SkImage.h"
#include "include/core/SkImageGenerator.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkM44.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPath.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkPoint.h"
#include "include/core/SkPoint3.h"
#include "include/core/SkRRect.h"
#include "include/core/SkRect.h"
#include "include/core/SkRegion.h"
#include "include/core/SkSamplingOptions.h"
#include "include/core/SkSize.h"
#include "include/core/SkString.h"
#include "include/core/SkTypeface.h"
#include "include/private/SkAlign.h"
#include "include/private/SkAssert.h"
#include "include/private/SkDebug.h"
#include "include/private/SkFloatingPoint.h"
#include "include/private/SkMalloc.h"
#include "include/private/SkOnlyStackAllocatable.h"
#include "include/private/SkTArray.h"
#include "include/private/SkTFitsIn.h"
#include "include/private/SkTPin.h"
#include "include/private/SkTo.h"
#include "src/core/SkAutoMalloc.h"
#include "src/core/SkBuffer.h"
#include "src/core/SkFloatBits.h"
#include "src/core/SkMathPriv.h"
#include "src/core/SkMatrixPriv.h"
#include "src/core/SkMipmap.h"
#include "src/core/SkMipmapBuilder.h"
#include "src/core/SkPaintPriv.h"
#include "src/core/SkPtrRecorder.h"
#include "src/core/SkReadBuffer.h"
#include "src/core/SkSafeMath.h"
#include "src/core/SkStringUtils.h"
#include "src/core/SkUTF.h"
#include "src/core/SkUtils.h"
#include "src/core/SkWriteBuffer.h"
#include "src/core/SkWriter32.h"
#include "src/image/SkImage_Base.h"

#include <algorithm>
#include <cstdarg>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <memory>
#include <new>
#include <optional>
#include <string_view>
#include <utility>

using namespace skia_private;

extern "C" {

// SkBuffer
bool skia_rs_rbuffer_skip(bool valid, size_t available, size_t size, size_t* out_advance);
bool skia_rs_rbuffer_skip_to_align4(uintptr_t pos_addr, bool valid, size_t available,
                                    size_t* out_advance);
size_t skia_rs_wbuffer_pad_align4(size_t pos);

// SkReadBuffer / SkWriteBuffer / SkWriter32
bool skia_rs_read_buffer_validate_memory(uintptr_t addr, size_t size);
bool skia_rs_read_buffer_skip(bool already_error, uintptr_t curr_addr, size_t available,
                              size_t size, size_t* out_inc);
bool skia_rs_read_buffer_skip_mul(bool already_error, uintptr_t curr_addr, size_t available,
                                  size_t count, size_t size, size_t* out_inc);
bool skia_rs_read_buffer_validate_bool(uint32_t value, bool* out_valid);
int32_t skia_rs_read_buffer_check_int(int32_t value, int32_t min, int32_t max, bool* out_valid);
size_t skia_rs_negate_to_size_t(int32_t value);
uint32_t skia_rs_encode_flattenable_dict_index(uint32_t index);
uint32_t skia_rs_decode_flattenable_dict_index(uint32_t word);
size_t skia_rs_writer32_string_size(size_t len);
size_t skia_rs_writer32_grow_capacity(size_t current_capacity, size_t needed_size);
size_t skia_rs_writer32_format_string(const uint8_t* src, size_t src_len,
                                      uint8_t* dst, size_t dst_cap);
size_t skia_rs_writer32_encode_sampling(int32_t max_aniso, bool use_cubic,
                                        float cubic_b, float cubic_c,
                                        uint32_t filter, uint32_t mipmap,
                                        uint32_t out_words[4]);

// SkString & SkStringUtils
bool skia_rs_str_ends_with(const uint8_t* s, size_t s_len,
                           const uint8_t* suffix, size_t suffix_len);
bool skia_rs_str_ends_with_char(const uint8_t* s, size_t s_len, uint8_t suffix_char);
int32_t skia_rs_str_starts_with_one_of(const uint8_t* s, size_t s_len, const uint8_t* prefixes);
size_t skia_rs_str_append_u32(uint8_t* out, uint32_t dec);
size_t skia_rs_str_append_s32(uint8_t* out, int32_t dec);
size_t skia_rs_str_append_u64(uint8_t* out, uint64_t dec, int32_t min_digits);
size_t skia_rs_str_append_s64(uint8_t* out, int64_t dec, int32_t min_digits);
size_t skia_rs_str_append_scalar(uint8_t* out, float value);
size_t skia_rs_string_format_hex_u32(uint32_t hex, int32_t min_digits, uint8_t out[8]);
size_t skia_rs_string_format_append_scalar(float value, bool as_hex, uint8_t* out, size_t out_cap);
uint32_t skia_rs_trim_size_t_to_u32(size_t value);
size_t skia_rs_check_add32(size_t base, size_t extra);
bool skia_rs_string_rec_alloc_size(size_t rec_header_size, size_t len,
                                   uint32_t* out_string_len, size_t* out_alloc_size);
bool skia_rs_string_can_reuse_rec_for_resize(bool is_unique, uint32_t current_len, uint32_t new_len);
bool skia_rs_string_can_insert_in_place(bool is_unique, size_t current_len, size_t insert_len);
void skia_rs_string_clamp_remove_range(size_t size, size_t offset, size_t length,
                                       size_t* out_actual_len, size_t* out_tail);
void skia_rs_string_tab_string(const uint8_t* input, size_t input_len, int32_t tab_cnt,
                               void (*cb)(const uint8_t*, size_t, void*), void* ctx);
size_t skia_rs_string_utf16_to_utf8_clamped_len(const uint16_t* src, size_t count,
                                                size_t* out_units_used);
size_t skia_rs_string_utf16_to_utf8_write(const uint16_t* src, size_t units_used,
                                          uint8_t* dst, size_t dst_len);
void skia_rs_string_split_ranges(const uint8_t* input, size_t input_len,
                                 const uint8_t* delims, size_t delims_len,
                                 bool strict,
                                 void (*cb)(size_t, size_t, void*), void* ctx);

}  // extern "C"

// ============================================================================
// SkBuffer (SkRBuffer & SkWBuffer)
// ============================================================================

const void* SkRBuffer::skip(size_t size) {
    size_t advance = 0;
    if (skia_rs_rbuffer_skip(fValid, this->available(), size, &advance)) {
        const void* pos = fPos;
        fPos += advance;
        return pos;
    }
    fValid = false;
    return nullptr;
}

bool SkRBuffer::read(void* buffer, size_t size) {
    if (const void* src = this->skip(size)) {
        sk_careful_memcpy(buffer, src, size);
        return true;
    }
    return false;
}

bool SkRBuffer::skipToAlign4() {
    size_t advance = 0;
    if (skia_rs_rbuffer_skip_to_align4(reinterpret_cast<uintptr_t>(fPos), fValid,
                                       this->available(), &advance)) {
        fPos += advance;
        return true;
    }
    fValid = false;
    return false;
}

void* SkWBuffer::skip(size_t size) {
    void* result = fPos;
    writeNoSizeCheck(nullptr, size);
    return fData == nullptr ? nullptr : result;
}

void SkWBuffer::writeNoSizeCheck(const void* buffer, size_t size) {
    SkASSERT(fData == nullptr || fStop == nullptr || fPos + size <= fStop);
    if (fData && buffer) {
        sk_careful_memcpy(fPos, buffer, size);
    }
    fPos += size;
}

size_t SkWBuffer::padToAlign4() {
    size_t n = skia_rs_wbuffer_pad_align4(this->pos());
    if (n && fData) {
        memset(fPos, 0, n);
    }
    fPos += n;
    return n;
}

// ============================================================================
// SkReadBuffer
// ============================================================================

namespace {
class EmptyImageGenerator final : public SkImageGenerator {
public:
    EmptyImageGenerator(const SkImageInfo& info) : SkImageGenerator(info) {}
};

static sk_sp<SkImage> MakeEmptyImage(int width, int height) {
    return SkImages::DeferredFromGenerator(
            std::make_unique<EmptyImageGenerator>(SkImageInfo::MakeN32Premul(width, height)));
}
}  // anonymous namespace

void SkReadBuffer::setMemory(const void* data, size_t size) {
    this->validate(skia_rs_read_buffer_validate_memory(reinterpret_cast<uintptr_t>(data), size));
    if (!fError) {
        fBase = fCurr = (const char*)data;
        fStop = fBase + size;
    }
}

void SkReadBuffer::setInvalid() {
    if (!fError) {
        fCurr = fStop;
        fError = true;
    }
}

const void* SkReadBuffer::skip(size_t size) {
    size_t inc = 0;
    if (!skia_rs_read_buffer_skip(fError, reinterpret_cast<uintptr_t>(fCurr),
                                  this->available(), size, &inc)) {
        this->setInvalid();
        return nullptr;
    }
    const void* addr = fCurr;
    fCurr += inc;
    return addr;
}

const void* SkReadBuffer::skip(size_t count, size_t size) {
    size_t inc = 0;
    if (!skia_rs_read_buffer_skip_mul(fError, reinterpret_cast<uintptr_t>(fCurr),
                                      this->available(), count, size, &inc)) {
        this->setInvalid();
        return nullptr;
    }
    const void* addr = fCurr;
    fCurr += inc;
    return addr;
}

void SkReadBuffer::setDeserialProcs(const SkDeserialProcs& procs) {
    fProcs = procs;
    this->setAllowSkSL(procs.fAllowSkSL);
}

bool SkReadBuffer::readBool() {
    uint32_t value = this->readUInt();
    bool valid = false;
    bool res = skia_rs_read_buffer_validate_bool(value, &valid);
    this->validate(valid);
    return res;
}

SkColor SkReadBuffer::readColor() {
    return this->readUInt();
}

int32_t SkReadBuffer::readInt() {
    const size_t inc = sizeof(int32_t);
    if (!this->validate(IsPtrAlign4(fCurr) && this->isAvailable(inc))) {
        return 0;
    }
    int32_t value = *((const int32_t*)fCurr);
    fCurr += inc;
    return value;
}

SkScalar SkReadBuffer::readScalar() {
    const size_t inc = sizeof(SkScalar);
    if (!this->validate(IsPtrAlign4(fCurr) && this->isAvailable(inc))) {
        return 0;
    }
    SkScalar value = *((const SkScalar*)fCurr);
    fCurr += inc;
    return value;
}

uint32_t SkReadBuffer::readUInt() {
    return this->readInt();
}

int32_t SkReadBuffer::read32() {
    return this->readInt();
}

uint8_t SkReadBuffer::peekByte() {
    if (this->available() <= 0) {
        fError = true;
        return 0;
    }
    return *((const uint8_t*)fCurr);
}

bool SkReadBuffer::readPad32(void* buffer, size_t bytes) {
    if (const void* src = this->skip(bytes)) {
        sk_careful_memcpy(buffer, src, bytes);
        return true;
    }
    return false;
}

const char* SkReadBuffer::readString(size_t* len) {
    *len = this->readUInt();
    const char* c_str = this->skipT<char>(*len + 1);
    if (this->validate(c_str && c_str[*len] == '\0')) {
        return c_str;
    }
    return nullptr;
}

void SkReadBuffer::readString(SkString* string) {
    size_t len;
    if (const char* c_str = this->readString(&len)) {
        string->set(c_str, len);
        return;
    }
    string->reset();
}

void SkReadBuffer::readColor4f(SkColor4f* color) {
    if (!this->readPad32(color, sizeof(SkColor4f))) {
        *color = {0, 0, 0, 0};
    }
}

void SkReadBuffer::readPoint(SkPoint* point) {
    point->fX = this->readScalar();
    point->fY = this->readScalar();
}

void SkReadBuffer::readPoint3(SkPoint3* point) {
    this->readPad32(point, sizeof(SkPoint3));
}

void SkReadBuffer::read(SkM44* matrix) {
    if (this->isValid()) {
        if (const float* m = (const float*)this->skip(sizeof(float) * 16)) {
            *matrix = SkM44::ColMajor(m);
        }
    }
    if (!this->isValid()) {
        *matrix = SkM44();
    }
}

void SkReadBuffer::readMatrix(SkMatrix* matrix) {
    size_t size = 0;
    if (this->isValid()) {
        size = SkMatrixPriv::ReadFromMemory(matrix, fCurr, this->available());
        (void)this->validate((SkAlign4(size) == size) && (0 != size));
    }
    if (!this->isValid()) {
        matrix->reset();
    }
    (void)this->skip(size);
}

void SkReadBuffer::readIRect(SkIRect* rect) {
    if (!this->readPad32(rect, sizeof(SkIRect))) {
        rect->setEmpty();
    }
}

void SkReadBuffer::readRect(SkRect* rect) {
    if (!this->readPad32(rect, sizeof(SkRect))) {
        rect->setEmpty();
    }
}

SkRect SkReadBuffer::readRect() {
    SkRect r;
    if (!this->readPad32(&r, sizeof(SkRect))) {
        r.setEmpty();
    }
    return r;
}

SkSamplingOptions SkReadBuffer::readSampling() {
    if (!this->isVersionLT(SkPicturePriv::kAnisotropicFilter)) {
        int maxAniso = this->readInt();
        if (maxAniso != 0) {
            return SkSamplingOptions::Aniso(maxAniso);
        }
    }
    if (this->readBool()) {
        float B = this->readScalar();
        float C = this->readScalar();
        return SkSamplingOptions({B, C});
    } else {
        SkFilterMode filter = this->read32LE(SkFilterMode::kLinear);
        SkMipmapMode mipmap = this->read32LE(SkMipmapMode::kLinear);
        return SkSamplingOptions(filter, mipmap);
    }
}

void SkReadBuffer::readRRect(SkRRect* rrect) {
    size_t size = 0;
    if (!fError) {
        size = rrect->readFromMemory(fCurr, this->available());
        if (!this->validate((SkAlign4(size) == size) && (0 != size))) {
            rrect->setEmpty();
        }
    }
    (void)this->skip(size);
}

void SkReadBuffer::readRegion(SkRegion* region) {
    size_t size = 0;
    if (!fError) {
        size = region->readFromMemory(fCurr, this->available());
        if (!this->validate((SkAlign4(size) == size) && (0 != size))) {
            region->setEmpty();
        }
    }
    (void)this->skip(size);
}

std::optional<SkPath> SkReadBuffer::readPath() {
    if (fError) {
        return {};
    }
    size_t size = 0;
    auto path = SkPath::ReadFromMemory(fCurr, this->available(), &size);
    (void)this->validate(SkAlign4(size) == size && path.has_value());
    (void)this->skip(size);
    return path;
}

bool SkReadBuffer::readArray(void* value, size_t size, size_t elementSize) {
    const uint32_t count = this->readUInt();
    return this->validate(size == count) &&
           this->readPad32(value, SkSafeMath::Mul(size, elementSize));
}

bool SkReadBuffer::readByteArray(void* value, size_t size) {
    return this->readArray(value, size, sizeof(uint8_t));
}

bool SkReadBuffer::readColorArray(SkSpan<SkColor> colors) {
    return this->readArray(colors.data(), colors.size(), sizeof(SkColor));
}

bool SkReadBuffer::readColor4fArray(SkSpan<SkColor4f> colors) {
    return this->readArray(colors.data(), colors.size(), sizeof(SkColor4f));
}

bool SkReadBuffer::readIntArray(SkSpan<int32_t> values) {
    return this->readArray(values.data(), values.size(), sizeof(int32_t));
}

bool SkReadBuffer::readPointArray(SkSpan<SkPoint> points) {
    return this->readArray(points.data(), points.size(), sizeof(SkPoint));
}

bool SkReadBuffer::readScalarArray(SkSpan<SkScalar> values) {
    return this->readArray(values.data(), values.size(), sizeof(SkScalar));
}

const void* SkReadBuffer::skipByteArray(size_t* size) {
    const uint32_t count = this->readUInt();
    const void* buf = this->skip(count);
    if (size) {
        *size = this->isValid() ? count : 0;
    }
    return buf;
}

sk_sp<SkData> SkReadBuffer::readByteArrayAsData() {
    size_t numBytes = this->getArrayCount();
    if (!this->validate(this->isAvailable(numBytes))) {
        return nullptr;
    }
    SkAutoMalloc buffer(numBytes);
    if (!this->readByteArray(buffer.get(), numBytes)) {
        return nullptr;
    }
    return SkData::MakeFromMalloc(buffer.release(), numBytes);
}

uint32_t SkReadBuffer::getArrayCount() {
    const size_t inc = sizeof(uint32_t);
    if (!this->validate(IsPtrAlign4(fCurr) && this->isAvailable(inc))) {
        return 0;
    }
    return *((const uint32_t*)fCurr);
}

static sk_sp<SkImage> deserialize_image(sk_sp<SkData> data, SkDeserialProcs dProcs,
                                        std::optional<SkAlphaType> alphaType) {
    sk_sp<SkImage> image;
    if (dProcs.fImageDataProc) {
        image = dProcs.fImageDataProc(data, alphaType, dProcs.fImageCtx);
    } else if (dProcs.fImageProc) {
        image = dProcs.fImageProc(data->data(), data->size(), alphaType, dProcs.fImageCtx);
    }
    return image;
}

static sk_sp<SkImage> add_mipmaps(sk_sp<SkImage> img, sk_sp<SkData> data,
                                  SkDeserialProcs dProcs, std::optional<SkAlphaType> alphaType) {
    SkMipmapBuilder builder(img->imageInfo());
    SkReadBuffer buffer(data->data(), data->size());
    int count = buffer.read32();
    if (builder.countLevels() != count) {
        return img;
    }
    for (int i = 0; i < count; ++i) {
        size_t size = buffer.read32();
        const void* ptr = buffer.skip(size);
        if (!ptr) {
            return img;
        }
        sk_sp<SkImage> mip = deserialize_image(SkData::MakeWithoutCopy(ptr, size), dProcs,
                                               alphaType);
        if (!mip) {
            return img;
        }
        SkPixmap pm = builder.level(i);
        if (mip->dimensions() != pm.dimensions()) {
            return img;
        }
        if (!mip->readPixels(nullptr, pm, 0, 0)) {
            return img;
        }
    }
    if (!buffer.isValid()) {
        return img;
    }
    sk_sp<SkImage> raster = img->makeRasterImage(nullptr);
    if (!raster) {
        return img;
    }
    sk_sp<SkImage> rasterWithMips = builder.attachTo(raster);
    SkASSERT(rasterWithMips);
    return rasterWithMips;
}

sk_sp<SkImage> SkReadBuffer::readImage() {
    uint32_t flags = this->read32();

    std::optional<SkAlphaType> alphaType = std::nullopt;
    if (flags & SkWriteBufferImageFlags::kUnpremul) {
        alphaType = kUnpremul_SkAlphaType;
    }
    sk_sp<SkImage> image;
    {
        sk_sp<SkData> data = this->readByteArrayAsData();
        if (!data) {
            this->validate(false);
            return nullptr;
        }
        image = deserialize_image(data, fProcs, alphaType);
    }

    if (flags & SkWriteBufferImageFlags::kHasSubsetRect) {
        SkIRect subset;
        this->readIRect(&subset);
        if (image) {
            image = image->makeSubset(nullptr, subset, {});
        }
    }

    if (flags & SkWriteBufferImageFlags::kHasMipmap) {
        sk_sp<SkData> data = this->readByteArrayAsData();
        if (!data) {
            this->validate(false);
            return nullptr;
        }
        if (image) {
            image = add_mipmaps(image, std::move(data), fProcs, alphaType);
        }
    }
    return image ? image : MakeEmptyImage(1, 1);
}

sk_sp<SkTypeface> SkReadBuffer::readTypeface() {
    int32_t index = this->read32();
    if (index == 0) {
        return nullptr;
    } else if (index > 0) {
        if (!this->validate(index <= fTFCount)) {
            return nullptr;
        }
        return fTFArray[index - 1];
    } else {
        size_t size = skia_rs_negate_to_size_t(index);
        const void* data = this->skip(size);
        if (!this->validate(data && fProcs.fTypefaceStreamProc)) {
            return nullptr;
        }
        SkMemoryStream s(data, size, false);
        return fProcs.fTypefaceStreamProc(s, fProcs.fTypefaceCtx);
    }
}

SkFlattenable* SkReadBuffer::readRawFlattenable() {
    if (!this->isValid()) {
        return nullptr;
    }
    SkFlattenable::Factory factory = nullptr;

    if (fFactoryCount > 0) {
        int32_t index = this->read32();
        if (0 == index || !this->isValid()) {
            return nullptr;
        }
        if (index < 0) {
            this->validate(false);
            return nullptr;
        }
        index -= 1;
        if ((unsigned)index >= (unsigned)fFactoryCount) {
            this->validate(false);
            return nullptr;
        }
        factory = fFactoryArray[index];
    } else {
        if (this->peekByte() != 0) {
            size_t ignored_length;
            if (const char* name = this->readString(&ignored_length)) {
                factory = SkFlattenable::NameToFactory(name);
                fFlattenableDict.set(fFlattenableDict.count() + 1, factory);
            }
        } else {
            uint32_t index = skia_rs_decode_flattenable_dict_index(this->readUInt());
            if (index == 0) {
                return nullptr;
            }
            if (SkFlattenable::Factory* found = fFlattenableDict.find(index)) {
                factory = *found;
            }
        }

        if (!this->validate(factory != nullptr)) {
            return nullptr;
        }
    }

    sk_sp<SkFlattenable> obj;
    uint32_t sizeRecorded = this->read32();
    if (factory) {
        size_t offset = this->offset();
        obj = (*factory)(*this);
        size_t sizeRead = this->offset() - offset;
        if (sizeRecorded != sizeRead) {
            this->validate(false);
            return nullptr;
        }
    } else {
        this->skip(sizeRecorded);
    }
    if (!this->isValid()) {
        return nullptr;
    }
    return obj.release();
}

SkFlattenable* SkReadBuffer::readFlattenable(SkFlattenable::Type ft) {
    SkFlattenable* obj = this->readRawFlattenable();
    if (obj && obj->getFlattenableType() != ft) {
        this->validate(false);
        obj->unref();
        return nullptr;
    }
    return obj;
}

int32_t SkReadBuffer::checkInt(int32_t min, int32_t max) {
    SkASSERT(min <= max);
    int32_t value = this->read32();
    bool valid = false;
    int32_t res = skia_rs_read_buffer_check_int(value, min, max, &valid);
    if (!valid) {
        this->validate(false);
    }
    return res;
}

SkLegacyFQ SkReadBuffer::checkFilterQuality() {
    return this->checkRange<SkLegacyFQ>(kNone_SkLegacyFQ, kLast_SkLegacyFQ);
}

// ============================================================================
// SkBinaryWriteBuffer (SkWriteBuffer.cpp)
// ============================================================================

SkBinaryWriteBuffer::SkBinaryWriteBuffer(const SkSerialProcs& p)
        : SkWriteBuffer(p), fFactorySet(nullptr), fTFSet(nullptr) {}

SkBinaryWriteBuffer::SkBinaryWriteBuffer(void* storage, size_t storageSize, const SkSerialProcs& p)
        : SkWriteBuffer(p), fFactorySet(nullptr), fTFSet(nullptr), fWriter(storage, storageSize) {}

SkBinaryWriteBuffer::~SkBinaryWriteBuffer() {}

bool SkBinaryWriteBuffer::usingInitialStorage() const {
    return fWriter.usingInitialStorage();
}

void SkBinaryWriteBuffer::writeByteArray(const void* data, size_t size) {
    fWriter.write32(SkToU32(size));
    fWriter.writePad(data, size);
}

void SkBinaryWriteBuffer::writeBool(bool value) {
    fWriter.writeBool(value);
}

void SkBinaryWriteBuffer::writeScalar(SkScalar value) {
    fWriter.writeScalar(value);
}

void SkBinaryWriteBuffer::writeScalarArray(SkSpan<const SkScalar> values) {
    fWriter.write32(SkToInt(values.size()));
    fWriter.write(values.data(), values.size_bytes());
}

void SkBinaryWriteBuffer::writeInt(int32_t value) {
    fWriter.write32(value);
}

void SkBinaryWriteBuffer::writeIntArray(SkSpan<const int32_t> values) {
    fWriter.write32(SkToInt(values.size()));
    fWriter.write(values.data(), values.size_bytes());
}

void SkBinaryWriteBuffer::writeUInt(uint32_t value) {
    fWriter.write32(value);
}

void SkBinaryWriteBuffer::writeString(std::string_view value) {
    fWriter.writeString(value.data(), value.size());
}

void SkBinaryWriteBuffer::writeColor(SkColor color) {
    fWriter.write32(color);
}

void SkBinaryWriteBuffer::writeColorArray(SkSpan<const SkColor> values) {
    fWriter.write32(SkToInt(values.size()));
    fWriter.write(values.data(), values.size_bytes());
}

void SkBinaryWriteBuffer::writeColor4f(const SkColor4f& color) {
    fWriter.write(&color, sizeof(SkColor4f));
}

void SkBinaryWriteBuffer::writeColor4fArray(SkSpan<const SkColor4f> values) {
    fWriter.write32(SkToInt(values.size()));
    fWriter.write(values.data(), values.size_bytes());
}

void SkBinaryWriteBuffer::writePoint(const SkPoint& point) {
    fWriter.writeScalar(point.fX);
    fWriter.writeScalar(point.fY);
}

void SkBinaryWriteBuffer::writePoint3(const SkPoint3& point) {
    this->writePad32(&point, sizeof(SkPoint3));
}

void SkBinaryWriteBuffer::writePointArray(SkSpan<const SkPoint> values) {
    fWriter.write32(SkToInt(values.size()));
    fWriter.write(values.data(), values.size_bytes());
}

void SkBinaryWriteBuffer::write(const SkM44& matrix) {
    fWriter.write(SkMatrixPriv::M44ColMajor(matrix), sizeof(float) * 16);
}

void SkBinaryWriteBuffer::writeMatrix(const SkMatrix& matrix) {
    fWriter.writeMatrix(matrix);
}

void SkBinaryWriteBuffer::writeIRect(const SkIRect& rect) {
    fWriter.write(&rect, sizeof(SkIRect));
}

void SkBinaryWriteBuffer::writeRect(const SkRect& rect) {
    fWriter.writeRect(rect);
}

void SkBinaryWriteBuffer::writeRegion(const SkRegion& region) {
    fWriter.writeRegion(region);
}

void SkBinaryWriteBuffer::writeSampling(const SkSamplingOptions& sampling) {
    fWriter.writeSampling(sampling);
}

void SkBinaryWriteBuffer::writePath(const SkPath& path) {
    fWriter.writePath(path);
}

size_t SkBinaryWriteBuffer::writeStream(SkStream* stream, size_t length) {
    fWriter.write32(SkToU32(length));
    size_t bytesWritten = fWriter.readFromStream(stream, length);
    if (bytesWritten < length) {
        fWriter.reservePad(length - bytesWritten);
    }
    return bytesWritten;
}

bool SkBinaryWriteBuffer::writeToStream(SkWStream* stream) const {
    return fWriter.writeToStream(stream);
}

static sk_sp<const SkData> serialize_image(const SkImage* image, SkSerialProcs procs) {
    sk_sp<const SkData> data;
    if (procs.fImageProc) {
        data = procs.fImageProc(const_cast<SkImage*>(image), procs.fImageCtx);
    }
    if (data) {
        return data;
    }
    data = image->refEncodedData();
    if (data) {
        return data;
    }
    return nullptr;
}

static sk_sp<SkData> serialize_mipmap(const SkMipmap* mipmap, SkSerialProcs procs) {
    const int count = mipmap->countLevels();
    SkBinaryWriteBuffer buffer({});
    buffer.write32(count);
    for (int i = 0; i < count; ++i) {
        SkMipmap::Level level;
        if (mipmap->getLevel(i, &level)) {
            sk_sp<SkImage> levelImage = SkImages::RasterFromPixmap(level.fPixmap, nullptr, nullptr);
            sk_sp<const SkData> levelData = serialize_image(levelImage.get(), procs);
            buffer.writeDataAsByteArray(levelData.get());
        } else {
            return nullptr;
        }
    }
    return buffer.snapshotAsData();
}

void SkBinaryWriteBuffer::writeImage(const SkImage* image) {
    uint32_t flags = 0;
    const SkMipmap* mips = as_IB(image)->onPeekMips();
    if (mips) {
        flags |= SkWriteBufferImageFlags::kHasMipmap;
    }
    if (image->alphaType() == kUnpremul_SkAlphaType) {
        flags |= SkWriteBufferImageFlags::kUnpremul;
    }

    this->write32(flags);

    sk_sp<const SkData> data = serialize_image(image, fProcs);
    SkASSERT(data);
    this->writeDataAsByteArray(data.get());

    if (flags & SkWriteBufferImageFlags::kHasMipmap) {
        sk_sp<SkData> mipData = serialize_mipmap(mips, fProcs);
        this->writeDataAsByteArray(mipData.get());
    }
}

void SkBinaryWriteBuffer::writeTypeface(SkTypeface* obj) {
    if (obj == nullptr) {
        fWriter.write32(0);
    } else if (fProcs.fTypefaceProc) {
        auto data = fProcs.fTypefaceProc(obj, fProcs.fTypefaceCtx);
        if (data) {
            size_t size = data->size();
            if (!SkTFitsIn<int32_t>(size)) {
                size = 0;
            }
            int32_t ssize = SkToS32(size);
            fWriter.write32(-ssize);
            if (size) {
                this->writePad32(data->data(), size);
            }
            return;
        }
    }
    fWriter.write32(fTFSet ? fTFSet->add(obj) : 0);
}

void SkBinaryWriteBuffer::writePaint(const SkPaint& paint) {
    SkPaintPriv::Flatten(paint, *this);
}

void SkBinaryWriteBuffer::setFactoryRecorder(sk_sp<SkFactorySet> rec) {
    fFactorySet = std::move(rec);
}

void SkBinaryWriteBuffer::setTypefaceRecorder(sk_sp<SkRefCntSet> rec) {
    fTFSet = std::move(rec);
}

void SkBinaryWriteBuffer::writeFlattenable(const SkFlattenable* flattenable) {
    if (nullptr == flattenable) {
        this->write32(0);
        return;
    }

    if (SkFlattenable::Factory factory = flattenable->getFactory(); factory && fFactorySet) {
        this->write32(fFactorySet->add(factory));
    } else {
        const char* name = flattenable->getTypeName();
        SkASSERT(name);
        SkASSERT(0 != strcmp("", name));

        if (uint32_t* indexPtr = fFlattenableDict.find(name)) {
            SkASSERT(0 == *indexPtr >> 24);
            this->write32(skia_rs_encode_flattenable_dict_index(*indexPtr));
        } else {
            this->writeString(name);
            fFlattenableDict.set(name, fFlattenableDict.count() + 1);
        }
    }

    (void)fWriter.reserve(sizeof(uint32_t));
    size_t offset = fWriter.bytesWritten();
    flattenable->flatten(*this);
    size_t objSize = fWriter.bytesWritten() - offset;
    fWriter.overwriteTAt(offset - sizeof(uint32_t), SkToU32(objSize));
}

// ============================================================================
// SkWriter32
// ============================================================================

void SkWriter32::writeMatrix(const SkMatrix& matrix) {
    size_t size = SkMatrixPriv::WriteToMemory(matrix, nullptr);
    SkASSERT(SkAlign4(size) == size);
    SkMatrixPriv::WriteToMemory(matrix, this->reserve(size));
}

void SkWriter32::writeSampling(const SkSamplingOptions& sampling) {
    uint32_t words[4];
    size_t count = skia_rs_writer32_encode_sampling(
            sampling.maxAniso,
            sampling.useCubic,
            sampling.cubic.B,
            sampling.cubic.C,
            static_cast<uint32_t>(sampling.filter),
            static_cast<uint32_t>(sampling.mipmap),
            words);
    for (size_t i = 0; i < count; ++i) {
        this->write32(static_cast<int32_t>(words[i]));
    }
}

void SkWriter32::writeString(const char str[], size_t len) {
    if (nullptr == str) {
        str = "";
        len = 0;
    }
    if ((long)len < 0) {
        len = strlen(str);
    }

    size_t total = skia_rs_writer32_string_size(len);
    uint32_t* ptr = this->reserve(total);
    skia_rs_writer32_format_string(reinterpret_cast<const uint8_t*>(str), len,
                                   reinterpret_cast<uint8_t*>(ptr), total);
}

size_t SkWriter32::WriteStringSize(const char* str, size_t len) {
    if ((long)len < 0) {
        SkASSERT(str);
        len = strlen(str);
    }
    return skia_rs_writer32_string_size(len);
}

void SkWriter32::growToAtLeast(size_t size) {
    const bool wasExternal = (fExternal != nullptr) && (fData == fExternal);

    fCapacity = skia_rs_writer32_grow_capacity(fCapacity, size);
    fData = nullptr;
    fInternal.realloc(fCapacity);
    fData = fInternal.get();

    if (wasExternal) {
        memcpy(fData, fExternal, fUsed);
    }
}

sk_sp<SkData> SkWriter32::snapshotAsData() const {
    return SkData::MakeWithCopy(fData, fUsed);
}

// ============================================================================
// SkString
// ============================================================================

static const size_t kBufferSize = 1024;

struct StringBuffer {
    SK_ONLY_STACK_ALLOCATABLE;
    char* fText;
    int   fLength;
};

template <int SIZE>
static StringBuffer apply_format_string(const char* format, va_list args, char (&stackBuffer)[SIZE],
                                        SkString* heapBuffer) SK_PRINTF_LIKE(1, 0);

template <int SIZE>
static StringBuffer apply_format_string(const char* format, va_list args, char (&stackBuffer)[SIZE],
                                        SkString* heapBuffer) {
    va_list argsCopy;
    va_copy(argsCopy, args);
    int outLength = std::vsnprintf(stackBuffer, SIZE, format, args);
    if (outLength < 0) {
        SkDebugf("SkString: vsnprintf reported error.");
        va_end(argsCopy);
        return {stackBuffer, 0};
    }
    if (outLength < SIZE) {
        va_end(argsCopy);
        return {stackBuffer, outLength};
    }

    heapBuffer->set(nullptr, outLength);
    char* heapBufferDest = heapBuffer->data();
    SkDEBUGCODE(int checkLength =) std::vsnprintf(heapBufferDest, outLength + 1, format, argsCopy);
    SkASSERT(checkLength == outLength);
    va_end(argsCopy);
    return {heapBufferDest, outLength};
}

bool SkStrEndsWith(const char string[], const char suffixStr[]) {
    SkASSERT(string);
    SkASSERT(suffixStr);
    return skia_rs_str_ends_with(reinterpret_cast<const uint8_t*>(string), strlen(string),
                                 reinterpret_cast<const uint8_t*>(suffixStr), strlen(suffixStr));
}

bool SkStrEndsWith(const char string[], char suffixChar) {
    SkASSERT(string);
    return skia_rs_str_ends_with_char(reinterpret_cast<const uint8_t*>(string), strlen(string),
                                      static_cast<uint8_t>(suffixChar));
}

int SkStrStartsWithOneOf(const char string[], const char prefixes[]) {
    return skia_rs_str_starts_with_one_of(reinterpret_cast<const uint8_t*>(string),
                                          strlen(string),
                                          reinterpret_cast<const uint8_t*>(prefixes));
}

char* SkStrAppendU32(char string[], uint32_t dec) {
    size_t n = skia_rs_str_append_u32(reinterpret_cast<uint8_t*>(string), dec);
    return string + n;
}

char* SkStrAppendS32(char string[], int32_t dec) {
    size_t n = skia_rs_str_append_s32(reinterpret_cast<uint8_t*>(string), dec);
    return string + n;
}

char* SkStrAppendU64(char string[], uint64_t dec, int minDigits) {
    size_t n = skia_rs_str_append_u64(reinterpret_cast<uint8_t*>(string), dec, minDigits);
    return string + n;
}

char* SkStrAppendS64(char string[], int64_t dec, int minDigits) {
    size_t n = skia_rs_str_append_s64(reinterpret_cast<uint8_t*>(string), dec, minDigits);
    return string + n;
}

char* SkStrAppendScalar(char string[], SkScalar value) {
    size_t n = skia_rs_str_append_scalar(reinterpret_cast<uint8_t*>(string), value);
    return string + n;
}

const SkString::Rec SkString::gEmptyRec(0, 0);

#define SizeOfRec() (gEmptyRec.data() - (const char*)&gEmptyRec)

sk_sp<SkString::Rec> SkString::Rec::Make(const char text[], size_t len) {
    if (0 == len) {
        return sk_sp<SkString::Rec>(const_cast<Rec*>(&gEmptyRec));
    }

    uint32_t stringLen = 0;
    size_t allocationSize = 0;
    bool ok = skia_rs_string_rec_alloc_size(static_cast<size_t>(SizeOfRec()), len,
                                            &stringLen, &allocationSize);
    SkASSERT_RELEASE(ok);

    void* storage = ::operator new (allocationSize);
    sk_sp<Rec> rec(new (storage) Rec(stringLen, 1));
    if (text) {
        memcpy(rec->data(), text, len);
    }
    rec->data()[len] = 0;
    return rec;
}

void SkString::Rec::ref() const {
    if (this == &SkString::gEmptyRec) {
        return;
    }
    SkAssertResult(this->fRefCnt.fetch_add(+1, std::memory_order_relaxed));
}

void SkString::Rec::unref() const {
    if (this == &SkString::gEmptyRec) {
        return;
    }
    int32_t oldRefCnt = this->fRefCnt.fetch_add(-1, std::memory_order_acq_rel);
    SkASSERT(oldRefCnt);
    if (1 == oldRefCnt) {
        delete this;
    }
}

bool SkString::Rec::unique() const {
    return fRefCnt.load(std::memory_order_acquire) == 1;
}

#ifdef SK_DEBUG
int32_t SkString::Rec::getRefCnt() const {
    return fRefCnt.load(std::memory_order_relaxed);
}

const SkString& SkString::validate() const {
    SkASSERT(0 == gEmptyRec.fLength);
    SkASSERT(0 == gEmptyRec.getRefCnt());
    SkASSERT(0 == gEmptyRec.data()[0]);

    if (fRec.get() != &gEmptyRec) {
        SkASSERT(fRec->fLength > 0);
        SkASSERT(fRec->getRefCnt() > 0);
        SkASSERT(0 == fRec->data()[fRec->fLength]);
    }
    return *this;
}

SkString& SkString::validate() {
    const_cast<const SkString*>(this)->validate();
    return *this;
}
#endif

SkString::SkString() : fRec(const_cast<Rec*>(&gEmptyRec)) {}

SkString::SkString(size_t len) {
    fRec = Rec::Make(nullptr, len);
}

SkString::SkString(const char text[]) {
    size_t len = text ? strlen(text) : 0;
    fRec = Rec::Make(text, len);
}

SkString::SkString(const char text[], size_t len) {
    fRec = Rec::Make(text, len);
}

SkString::SkString(const SkString& src) : fRec(src.validate().fRec) {}

SkString::SkString(SkString&& src) : fRec(std::move(src.validate().fRec)) {
    src.fRec.reset(const_cast<Rec*>(&gEmptyRec));
}

SkString::SkString(const std::string& src) {
    fRec = Rec::Make(src.c_str(), src.size());
}

SkString::SkString(std::string_view src) {
    fRec = Rec::Make(src.data(), src.length());
}

SkString::~SkString() {
    this->validate();
}

bool SkString::equals(const SkString& src) const {
    return fRec == src.fRec || this->equals(src.c_str(), src.size());
}

bool SkString::equals(const char text[]) const {
    return this->equals(text, text ? strlen(text) : 0);
}

bool SkString::equals(const char text[], size_t len) const {
    SkASSERT(len == 0 || text != nullptr);
    return fRec->fLength == len && !sk_careful_memcmp(fRec->data(), text, len);
}

SkString& SkString::operator=(const SkString& src) {
    this->validate();
    fRec = src.fRec;
    return *this;
}

SkString& SkString::operator=(SkString&& src) {
    this->validate();
    if (fRec != src.fRec) {
        this->swap(src);
    }
    return *this;
}

SkString& SkString::operator=(const char text[]) {
    this->validate();
    return *this = SkString(text);
}

void SkString::reset() {
    this->validate();
    fRec.reset(const_cast<Rec*>(&gEmptyRec));
}

char* SkString::data() {
    this->validate();
    if (fRec->fLength) {
        if (!fRec->unique()) {
            fRec = Rec::Make(fRec->data(), fRec->fLength);
        }
    }
    return fRec->data();
}

void SkString::resize(size_t len) {
    uint32_t newLen = skia_rs_trim_size_t_to_u32(len);
    if (0 == newLen) {
        this->reset();
    } else if (skia_rs_string_can_reuse_rec_for_resize(fRec->unique(), fRec->fLength, newLen)) {
        char* p = this->data();
        p[newLen] = '\0';
        fRec->fLength = newLen;
    } else {
        SkString newString(newLen);
        char* dest = newString.data();
        uint32_t copyLen = std::min<uint32_t>(newLen, SkToU32(this->size()));
        memcpy(dest, this->c_str(), copyLen);
        dest[copyLen] = '\0';
        this->swap(newString);
    }
}

void SkString::set(const char text[]) {
    this->set(text, text ? strlen(text) : 0);
}

void SkString::set(const char text[], size_t len) {
    uint32_t newLen = skia_rs_trim_size_t_to_u32(len);
    if (0 == newLen) {
        this->reset();
    } else if (skia_rs_string_can_reuse_rec_for_resize(fRec->unique(), fRec->fLength, newLen)) {
        char* p = this->data();
        if (text) {
            memcpy(p, text, newLen);
        }
        p[newLen] = '\0';
        fRec->fLength = newLen;
    } else {
        SkString tmp(text, newLen);
        this->swap(tmp);
    }
}

void SkString::insert(size_t offset, const char text[]) {
    this->insert(offset, text, text ? strlen(text) : 0);
}

void SkString::insert(size_t offset, const char text[], size_t len) {
    if (len) {
        size_t length = fRec->fLength;
        if (offset > length) {
            offset = length;
        }

        len = skia_rs_check_add32(length, len);
        if (0 == len) {
            return;
        }

        if (skia_rs_string_can_insert_in_place(fRec->unique(), length, len)) {
            char* dst = this->data();
            if (offset < length) {
                memmove(dst + offset + len, dst + offset, length - offset);
            }
            memcpy(dst + offset, text, len);
            dst[length + len] = 0;
            fRec->fLength = SkToU32(length + len);
        } else {
            SkString tmp(fRec->fLength + len);
            char* dst = tmp.data();
            if (offset > 0) {
                memcpy(dst, fRec->data(), offset);
            }
            memcpy(dst + offset, text, len);
            if (offset < fRec->fLength) {
                memcpy(dst + offset + len, fRec->data() + offset, fRec->fLength - offset);
            }
            this->swap(tmp);
        }
    }
}

void SkString::insertUnichar(size_t offset, SkUnichar uni) {
    char buffer[SkUTF::kMaxBytesInUTF8Sequence];
    size_t len = SkUTF::ToUTF8(uni, buffer);
    if (len) {
        this->insert(offset, buffer, len);
    }
}

void SkString::insertS32(size_t offset, int32_t dec) {
    char buffer[kSkStrAppendS32_MaxSize];
    char* stop = SkStrAppendS32(buffer, dec);
    this->insert(offset, buffer, stop - buffer);
}

void SkString::insertS64(size_t offset, int64_t dec, int minDigits) {
    char buffer[kSkStrAppendS64_MaxSize];
    char* stop = SkStrAppendS64(buffer, dec, minDigits);
    this->insert(offset, buffer, stop - buffer);
}

void SkString::insertU32(size_t offset, uint32_t dec) {
    char buffer[kSkStrAppendU32_MaxSize];
    char* stop = SkStrAppendU32(buffer, dec);
    this->insert(offset, buffer, stop - buffer);
}

void SkString::insertU64(size_t offset, uint64_t dec, int minDigits) {
    char buffer[kSkStrAppendU64_MaxSize];
    char* stop = SkStrAppendU64(buffer, dec, minDigits);
    this->insert(offset, buffer, stop - buffer);
}

void SkString::insertHex(size_t offset, uint32_t hex, int minDigits) {
    uint8_t buffer[8];
    size_t len = skia_rs_string_format_hex_u32(hex, minDigits, buffer);
    this->insert(offset, reinterpret_cast<const char*>(buffer), len);
}

void SkString::insertScalar(size_t offset, SkScalar value) {
    char buffer[kSkStrAppendScalar_MaxSize];
    char* stop = SkStrAppendScalar(buffer, value);
    this->insert(offset, buffer, stop - buffer);
}

void SkString::printf(const char format[], ...) {
    va_list args;
    va_start(args, format);
    this->printVAList(format, args);
    va_end(args);
}

void SkString::printVAList(const char format[], va_list args) {
    char stackBuffer[kBufferSize];
    StringBuffer result = apply_format_string(format, args, stackBuffer, this);
    if (result.fText == stackBuffer) {
        this->set(result.fText, result.fLength);
    }
}

void SkString::appendf(const char format[], ...) {
    va_list args;
    va_start(args, format);
    this->appendVAList(format, args);
    va_end(args);
}

void SkString::appendVAList(const char format[], va_list args) {
    if (this->isEmpty()) {
        this->printVAList(format, args);
        return;
    }
    SkString overflow;
    char stackBuffer[kBufferSize];
    StringBuffer result = apply_format_string(format, args, stackBuffer, &overflow);
    this->append(result.fText, result.fLength);
}

void SkString::prependf(const char format[], ...) {
    va_list args;
    va_start(args, format);
    this->prependVAList(format, args);
    va_end(args);
}

void SkString::prependVAList(const char format[], va_list args) {
    if (this->isEmpty()) {
        this->printVAList(format, args);
        return;
    }
    SkString overflow;
    char stackBuffer[kBufferSize];
    StringBuffer result = apply_format_string(format, args, stackBuffer, &overflow);
    this->prepend(result.fText, result.fLength);
}

void SkString::remove(size_t offset, size_t length) {
    size_t size = this->size();
    size_t actualLen = 0;
    size_t tail = 0;
    skia_rs_string_clamp_remove_range(size, offset, length, &actualLen, &tail);
    if (actualLen > 0) {
        SkString tmp(size - actualLen);
        char* dst = tmp.data();
        const char* src = this->c_str();
        if (offset) {
            memcpy(dst, src, offset);
        }
        if (tail) {
            memcpy(dst + offset, src + (offset + actualLen), tail);
        }
        SkASSERT(dst[tmp.size()] == 0);
        this->swap(tmp);
    }
}

void SkString::swap(SkString& other) {
    this->validate();
    other.validate();
    using std::swap;
    swap(fRec, other.fRec);
}

SkString SkStringPrintf(const char* format, ...) {
    SkString formattedOutput;
    va_list args;
    va_start(args, format);
    formattedOutput.printVAList(format, args);
    va_end(args);
    return formattedOutput;
}

// ============================================================================
// SkStringUtils
// ============================================================================

void SkAppendScalar(SkString* str, SkScalar value, SkScalarAsStringType asType) {
    uint8_t buf[64];
    size_t n = skia_rs_string_format_append_scalar(
            value, asType == kHex_SkScalarAsStringType, buf, sizeof(buf));
    str->append(reinterpret_cast<const char*>(buf), n);
}

SkString SkTabString(const SkString& string, int tabCnt) {
    if (tabCnt <= 0) {
        return string;
    }
    SkString result;
    skia_rs_string_tab_string(
            reinterpret_cast<const uint8_t*>(string.c_str()),
            string.size(),
            tabCnt,
            [](const uint8_t* data, size_t len, void* ctx) {
                static_cast<SkString*>(ctx)->set(reinterpret_cast<const char*>(data), len);
            },
            &result);
    return result;
}

SkString SkStringFromUTF16(const uint16_t* src, size_t count) {
    SkString ret;
    if (count > 0) {
        SkASSERT(src);
        size_t unitsUsed = 0;
        size_t n = skia_rs_string_utf16_to_utf8_clamped_len(src, count, &unitsUsed);
        ret = SkString(n);
        size_t written = skia_rs_string_utf16_to_utf8_write(
                src, unitsUsed, reinterpret_cast<uint8_t*>(ret.data()), n);
        SkASSERT(written == n);
        (void)written;
    }
    return ret;
}

void SkStrSplit(const char* str,
                const char* delimiters,
                SkStrSplitMode splitMode,
                TArray<SkString>* out) {
    struct SplitCtx {
        const char* base;
        TArray<SkString>* out;
    } ctx{str, out};

    skia_rs_string_split_ranges(
            reinterpret_cast<const uint8_t*>(str),
            strlen(str),
            reinterpret_cast<const uint8_t*>(delimiters),
            strlen(delimiters),
            splitMode == kStrict_SkStrSplitMode,
            [](size_t start, size_t len, void* rawCtx) {
                auto* c = static_cast<SplitCtx*>(rawCtx);
                c->out->push_back().set(c->base + start, len);
            },
            &ctx);
}
