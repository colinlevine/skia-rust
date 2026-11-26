/*
 * Safe Rust bridge for Layer 2.4 skia-rs-core-types:
 *   SkData.cpp, SkDataTable.cpp, SkStream.cpp.
 */

#include "include/core/SkData.h"
#include "include/core/SkDataTable.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkSpan.h"
#include "include/core/SkStream.h"
#include "include/core/SkString.h"
#include "include/core/SkTypes.h"
#include "include/private/SkAlign.h"
#include "include/private/SkAssert.h"
#include "include/private/SkDebug.h"
#include "include/private/SkMalloc.h"
#include "include/private/SkTFitsIn.h"
#include "include/private/SkTPin.h"
#include "include/private/SkTemplates.h"
#include "include/private/SkTo.h"
#include "src/core/SkOSFile.h"
#include "src/core/SkSafeMath.h"
#include "src/core/SkStreamPriv.h"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>
#include <memory>
#include <new>
#include <utility>
#include <vector>

extern "C" {

// SkData & SkDataTable
size_t skia_rs_data_copy_range(const uint8_t* src, size_t src_len, size_t offset, size_t length,
                               uint8_t* dst);
bool skia_rs_data_validate_subset(size_t size, size_t offset, size_t length);
bool skia_rs_data_inline_alloc_size(size_t obj_size, size_t length, size_t* out_actual);
bool skia_rs_data_table_copy_arrays_size(size_t dir_size, const size_t* sizes, size_t count,
                                         size_t* out_size);
bool skia_rs_data_table_copy_array_size(size_t elem_size, size_t count, size_t* out_size);
bool skia_rs_data_table_uniform_offset(size_t index, size_t elem_size, size_t* out_offset);

// SkStream & SkWStream
int32_t skia_rs_stream_size_of_packed_uint(size_t value);
size_t skia_rs_stream_encode_packed_uint(size_t value, uint8_t out[5]);
bool skia_rs_stream_decode_packed_uint(const uint8_t* bytes, size_t len,
                                       size_t* out_value, size_t* out_consumed);
void skia_rs_stream_file_clamp_init(size_t end, size_t start, size_t current,
                                    size_t* out_start, size_t* out_current);
size_t skia_rs_stream_file_seek(size_t start, size_t end, size_t position);
size_t skia_rs_stream_file_move(size_t start, size_t end, size_t current, int64_t offset);
size_t skia_rs_stream_clamp_read(size_t remaining, size_t requested);
size_t skia_rs_stream_memory_seek(size_t data_size, size_t position);
size_t skia_rs_stream_memory_move(size_t data_size, size_t current_offset, intptr_t delta);
size_t skia_rs_stream_dynamic_block_payload_size(size_t count, size_t block_header_size);
size_t skia_rs_stream_dynamic_pad_align4_bytes(size_t tail_written);
bool skia_rs_stream_remaining_length_is_below(bool has_length, size_t length,
                                              bool has_position, size_t position,
                                              size_t requested);

}  // extern "C"

// ============================================================================
// SkData
// ============================================================================

SkData::SkData(SkSpan<std::byte> span, ReleaseProc proc, void* context)
    : fReleaseProc(proc)
    , fReleaseProcContext(context)
    , fSpan(span)
{}

SkData::SkData(size_t size)
    : fReleaseProc(nullptr)
    , fReleaseProcContext(nullptr)
    , fSpan{(std::byte*)(this + 1), size}
{}

SkData::~SkData() {
    if (fReleaseProc) {
        fReleaseProc(fSpan.data(), fReleaseProcContext);
    }
}

bool SkData::operator==(const SkData& other) const {
    if (this == &other) {
        return true;
    }
    return size() == other.size() && !sk_careful_memcmp(data(), other.data(), size());
}

size_t SkData::copyRange(size_t offset, size_t length, void* buffer) const {
    return skia_rs_data_copy_range(this->bytes(), this->size(), offset, length,
                                   static_cast<uint8_t*>(buffer));
}

sk_sp<SkData> SkData::shareSubset(size_t offset, size_t length) {
    if (!skia_rs_data_validate_subset(this->size(), offset, length)) {
        return nullptr;
    }
    if (offset == 0 && length == this->size()) {
        return sk_ref_sp(this);
    }
    if (length == 0) {
        return SkData::MakeEmpty();
    }

    this->ref();
    return SkData::MakeWithProc(this->bytes() + offset, length, [](const void*, void* ctx) {
        ((SkData*)ctx)->unref();
    }, this);
}

sk_sp<const SkData> SkData::shareSubset(size_t offset, size_t length) const {
    return const_cast<SkData*>(this)->shareSubset(offset, length);
}

sk_sp<SkData> SkData::copySubset(size_t offset, size_t length) const {
    if (!skia_rs_data_validate_subset(this->size(), offset, length)) {
        return nullptr;
    }
    return SkData::MakeWithCopy(this->bytes() + offset, length);
}

void SkData::operator delete(void* p) {
    ::operator delete(p);
}

sk_sp<SkData> SkData::PrivateNewWithCopy(const void* srcOrNull, size_t length) {
    if (0 == length) {
        return SkData::MakeEmpty();
    }

    size_t actualLength = 0;
    bool ok = skia_rs_data_inline_alloc_size(sizeof(SkData), length, &actualLength);
    SkASSERT_RELEASE(ok);

    void* storage = ::operator new (actualLength);
    sk_sp<SkData> data(new (storage) SkData(length));
    if (srcOrNull) {
        memcpy(data->writable_data(), srcOrNull, length);
    }
    return data;
}

void SkData::NoopReleaseProc(const void*, void*) {}

sk_sp<SkData> SkData::MakeEmpty() {
    static SkData* empty = new SkData({}, nullptr, nullptr);
    return sk_ref_sp(empty);
}

static void sk_free_releaseproc(const void* ptr, void*) {
    sk_free(const_cast<void*>(ptr));
}

sk_sp<SkData> SkData::MakeFromMalloc(const void* data, size_t length) {
    std::byte* ptr = static_cast<std::byte*>(const_cast<void*>(data));
    return sk_sp<SkData>(new SkData({ptr, length}, sk_free_releaseproc, nullptr));
}

sk_sp<SkData> SkData::MakeWithCopy(const void* src, size_t length) {
    SkASSERT(src);
    return PrivateNewWithCopy(src, length);
}

sk_sp<SkData> SkData::MakeUninitialized(size_t length) {
    return PrivateNewWithCopy(nullptr, length);
}

sk_sp<SkData> SkData::MakeZeroInitialized(size_t length) {
    auto data = MakeUninitialized(length);
    if (length != 0) {
        memset(data->writable_data(), 0, data->size());
    }
    return data;
}

sk_sp<SkData> SkData::MakeWithProc(const void* data, size_t length, ReleaseProc proc, void* ctx) {
    std::byte* ptr = static_cast<std::byte*>(const_cast<void*>(data));
    return sk_sp<SkData>(new SkData({ptr, length}, proc, ctx));
}

static void sk_mmap_releaseproc(const void* addr, void* ctx) {
    size_t length = reinterpret_cast<size_t>(ctx);
    sk_fmunmap(addr, length);
}

sk_sp<SkData> SkData::MakeFromFILE(FILE* f) {
    size_t size;
    void* addr = sk_fmmap(f, &size);
    if (nullptr == addr) {
        return nullptr;
    }
    return SkData::MakeWithProc(addr, size, sk_mmap_releaseproc, reinterpret_cast<void*>(size));
}

sk_sp<SkData> SkData::MakeFromFileName(const char path[]) {
    FILE* f = path ? sk_fopen(path, kRead_SkFILE_Flag) : nullptr;
    if (nullptr == f) {
        return nullptr;
    }
    auto data = MakeFromFILE(f);
    sk_fclose(f);
    return data;
}

sk_sp<SkData> SkData::MakeFromFD(int fd) {
    size_t size;
    void* addr = sk_fdmmap(fd, &size);
    if (nullptr == addr) {
        return nullptr;
    }
    return SkData::MakeWithProc(addr, size, sk_mmap_releaseproc, reinterpret_cast<void*>(size));
}

sk_sp<SkData> SkData::MakeWithCString(const char cstr[]) {
    size_t size;
    if (nullptr == cstr) {
        cstr = "";
        size = 1;
    } else {
        size = strlen(cstr) + 1;
    }
    return MakeWithCopy(cstr, size);
}

sk_sp<SkData> SkData::MakeFromStream(SkStream* stream, size_t size) {
    if (SkStreamPriv::RemainingLengthIsBelow(stream, size)) {
        return nullptr;
    }
    sk_sp<SkData> data(SkData::MakeUninitialized(size));
    if (stream->read(data->writable_data(), size) != size) {
        return nullptr;
    }
    return data;
}

// ============================================================================
// SkDataTable
// ============================================================================

static void malloc_freeproc(void* context) {
    sk_free(context);
}

SkDataTable::SkDataTable() {
    fCount = 0;
    fElemSize = 0;
    fU.fDir = nullptr;
    fFreeProc = nullptr;
    fFreeProcContext = nullptr;
}

SkDataTable::SkDataTable(const void* array, size_t elemSize, int count,
                         FreeProc proc, void* context) {
    SkASSERT(count > 0);
    fCount = count;
    fElemSize = elemSize;
    fU.fElems = (const char*)array;
    fFreeProc = proc;
    fFreeProcContext = context;
}

SkDataTable::SkDataTable(const Dir* dir, int count, FreeProc proc, void* ctx) {
    SkASSERT(count > 0);
    fCount = count;
    fElemSize = 0;
    fU.fDir = dir;
    fFreeProc = proc;
    fFreeProcContext = ctx;
}

SkDataTable::~SkDataTable() {
    if (fFreeProc) {
        fFreeProc(fFreeProcContext);
    }
}

size_t SkDataTable::atSize(int index) const {
    SkASSERT((unsigned)index < (unsigned)fCount);
    if (fElemSize) {
        return fElemSize;
    } else {
        return fU.fDir[index].fSize;
    }
}

const void* SkDataTable::at(int index, size_t* size) const {
    SkASSERT((unsigned)index < (unsigned)fCount);
    if (fElemSize) {
        if (size) {
            *size = fElemSize;
        }
        size_t off = 0;
        SkAssertResult(skia_rs_data_table_uniform_offset(static_cast<size_t>(index), fElemSize, &off));
        return fU.fElems + off;
    } else {
        if (size) {
            *size = fU.fDir[index].fSize;
        }
        return fU.fDir[index].fPtr;
    }
}

sk_sp<SkDataTable> SkDataTable::MakeEmpty() {
    static SkDataTable* singleton = new SkDataTable();
    return sk_ref_sp(singleton);
}

sk_sp<SkDataTable> SkDataTable::MakeCopyArrays(const void* const* ptrs,
                                               const size_t sizes[], int count) {
    if (count <= 0) {
        return SkDataTable::MakeEmpty();
    }

    size_t bufferSize = 0;
    bool ok = skia_rs_data_table_copy_arrays_size(sizeof(Dir), sizes,
                                                  static_cast<size_t>(count), &bufferSize);
    SkASSERT_RELEASE(ok);
    void* buffer = sk_malloc_throw(bufferSize);

    Dir* dir = (Dir*)buffer;
    char* elem = (char*)(dir + count);
    for (int i = 0; i < count; ++i) {
        dir[i].fPtr = elem;
        dir[i].fSize = sizes[i];
        memcpy(elem, ptrs[i], sizes[i]);
        elem += sizes[i];
    }

    return sk_sp<SkDataTable>(new SkDataTable(dir, count, malloc_freeproc, buffer));
}

sk_sp<SkDataTable> SkDataTable::MakeCopyArray(const void* array, size_t elemSize, int count) {
    if (count <= 0) {
        return SkDataTable::MakeEmpty();
    }

    size_t bufferSize = 0;
    bool ok = skia_rs_data_table_copy_array_size(elemSize, static_cast<size_t>(count), &bufferSize);
    SkASSERT_RELEASE(ok);
    void* buffer = sk_malloc_throw(bufferSize);
    memcpy(buffer, array, bufferSize);

    return sk_sp<SkDataTable>(new SkDataTable(buffer, elemSize, count, malloc_freeproc, buffer));
}

sk_sp<SkDataTable> SkDataTable::MakeArrayProc(const void* array, size_t elemSize, int count,
                                              FreeProc proc, void* ctx) {
    if (count <= 0) {
        return SkDataTable::MakeEmpty();
    }
    return sk_sp<SkDataTable>(new SkDataTable(array, elemSize, count, proc, ctx));
}

// ============================================================================
// SkStream & SkWStream
// ============================================================================

bool SkStream::readS8(int8_t* i) {
    return this->read(i, sizeof(*i)) == sizeof(*i);
}

bool SkStream::readS16(int16_t* i) {
    return this->read(i, sizeof(*i)) == sizeof(*i);
}

bool SkStream::readS32(int32_t* i) {
    return this->read(i, sizeof(*i)) == sizeof(*i);
}

bool SkStream::readS64(int64_t* i) {
    return this->read(i, sizeof(*i)) == sizeof(*i);
}

bool SkStream::readScalar(SkScalar* i) {
    return this->read(i, sizeof(*i)) == sizeof(*i);
}

bool SkStream::readPackedUInt(size_t* i) {
    uint8_t buf[5];
    if (this->read(&buf[0], 1) != 1) {
        return false;
    }
    size_t total = 1;
    if (buf[0] == 0xFE) {
        if (this->read(&buf[1], 2) != 2) {
            return false;
        }
        total = 3;
    } else if (buf[0] == 0xFF) {
        if (this->read(&buf[1], 4) != 4) {
            return false;
        }
        total = 5;
    }
    size_t val = 0;
    size_t consumed = 0;
    if (!skia_rs_stream_decode_packed_uint(buf, total, &val, &consumed)) {
        return false;
    }
    *i = val;
    return true;
}

SkWStream::~SkWStream() {}

void SkWStream::flush() {}

bool SkWStream::writeDecAsText(int32_t dec) {
    char buffer[kSkStrAppendS32_MaxSize];
    char* stop = SkStrAppendS32(buffer, dec);
    return this->write(buffer, stop - buffer);
}

bool SkWStream::writeBigDecAsText(int64_t dec, int minDigits) {
    char buffer[kSkStrAppendU64_MaxSize];
    char* stop = SkStrAppendU64(buffer, dec, minDigits);
    return this->write(buffer, stop - buffer);
}

bool SkWStream::writeHexAsText(uint32_t hex, int digits) {
    SkString tmp;
    tmp.appendHex(hex, digits);
    return this->write(tmp.c_str(), tmp.size());
}

bool SkWStream::writeScalarAsText(SkScalar value) {
    char buffer[kSkStrAppendScalar_MaxSize];
    char* stop = SkStrAppendScalar(buffer, value);
    return this->write(buffer, stop - buffer);
}

bool SkWStream::writeScalar(SkScalar value) {
    return this->write(&value, sizeof(value));
}

int SkWStream::SizeOfPackedUInt(size_t value) {
    return skia_rs_stream_size_of_packed_uint(value);
}

bool SkWStream::writePackedUInt(size_t value) {
    uint8_t data[5];
    size_t len = skia_rs_stream_encode_packed_uint(value, data);
    return this->write(data, len);
}

bool SkWStream::writeStream(SkStream* stream, size_t length) {
    if (length == 0) {
        return true;
    }
    if (const char* base = static_cast<const char*>(stream->getMemoryBase());
        base && stream->hasPosition() && stream->hasLength()) {
        const size_t position = stream->getPosition();
        const size_t streamLength = stream->getLength();
        SkASSERT(streamLength >= position);
        if (position > streamLength || streamLength - position < length) {
            return false;
        }
        return stream->skip(length) == length && this->write(base + position, length);
    }
    char scratch[1024];
    const size_t MAX = sizeof(scratch);

    while (length != 0) {
        size_t n = skia_rs_stream_clamp_read(MAX, length);
        if (stream->read(scratch, n) != n || !this->write(scratch, n)) {
            return false;
        }
        length -= n;
    }
    return true;
}

// ============================================================================
// SkFILEStream
// ============================================================================

SkFILEStream::SkFILEStream(std::shared_ptr<FILE> file, size_t end, size_t start, size_t current)
    : fFILE(std::move(file))
    , fEnd(end)
    , fStart(0)
    , fCurrent(0)
{
    skia_rs_stream_file_clamp_init(end, start, current, &fStart, &fCurrent);
    SkASSERT(fStart == start);
    SkASSERT(fCurrent == current);
}

SkFILEStream::SkFILEStream(std::shared_ptr<FILE> file, size_t end, size_t start)
    : SkFILEStream(std::move(file), end, start, start)
{}

SkFILEStream::SkFILEStream(FILE* file, size_t size, size_t start)
    : SkFILEStream(std::shared_ptr<FILE>(file, sk_fclose), SkSafeMath::Add(start, size), start)
{}

SkFILEStream::SkFILEStream(FILE* file, size_t size)
    : SkFILEStream(file, size, file ? sk_ftell(file) : 0)
{}

SkFILEStream::SkFILEStream(FILE* file)
    : SkFILEStream(std::shared_ptr<FILE>(file, sk_fclose),
                   file ? sk_fgetsize(file) : 0,
                   file ? sk_ftell(file) : 0)
{}

SkFILEStream::SkFILEStream(const char path[])
    : SkFILEStream(path ? sk_fopen(path, kRead_SkFILE_Flag) : nullptr)
{}

SkFILEStream::~SkFILEStream() {
    this->close();
}

void SkFILEStream::close() {
    fFILE.reset();
    fEnd = 0;
    fStart = 0;
    fCurrent = 0;
}

size_t SkFILEStream::read(void* buffer, size_t size) {
    size = skia_rs_stream_clamp_read(fEnd - fCurrent, size);
    size_t bytesRead = size;
    if (buffer) {
        bytesRead = sk_qread(fFILE.get(), buffer, size, fCurrent);
    }
    if (bytesRead == SIZE_MAX) {
        return 0;
    }
    fCurrent += bytesRead;
    return bytesRead;
}

bool SkFILEStream::isAtEnd() const {
    if (fCurrent == fEnd) {
        return true;
    }
    return fCurrent >= sk_fgetsize(fFILE.get());
}

bool SkFILEStream::rewind() {
    fCurrent = fStart;
    return true;
}

SkStreamAsset* SkFILEStream::onDuplicate() const {
    return new SkFILEStream(fFILE, fEnd, fStart, fStart);
}

size_t SkFILEStream::getPosition() const {
    SkASSERT(fCurrent >= fStart);
    return fCurrent - fStart;
}

bool SkFILEStream::seek(size_t position) {
    fCurrent = skia_rs_stream_file_seek(fStart, fEnd, position);
    return true;
}

bool SkFILEStream::move(long offset) {
    fCurrent = skia_rs_stream_file_move(fStart, fEnd, fCurrent, static_cast<int64_t>(offset));
    SkASSERT(fCurrent >= fStart && fCurrent <= fEnd);
    return true;
}

SkStreamAsset* SkFILEStream::onFork() const {
    return new SkFILEStream(fFILE, fEnd, fStart, fCurrent);
}

size_t SkFILEStream::getLength() const {
    return fEnd - fStart;
}

// ============================================================================
// SkMemoryStream
// ============================================================================

static sk_sp<SkData> newFromParams(const void* src, size_t size, bool copyData) {
    if (copyData) {
        return SkData::MakeWithCopy(src, size);
    } else {
        return SkData::MakeWithoutCopy(src, size);
    }
}

SkMemoryStream::SkMemoryStream() {
    fData = SkData::MakeEmpty();
    fOffset = 0;
}

SkMemoryStream::SkMemoryStream(size_t size) {
    fData = SkData::MakeUninitialized(size);
    fOffset = 0;
}

SkMemoryStream::SkMemoryStream(const void* src, size_t size, bool copyData) {
    fData = newFromParams(src, size, copyData);
    fOffset = 0;
}

SkMemoryStream::SkMemoryStream(sk_sp<const SkData> data) : fData(std::move(data)) {
    if (nullptr == fData) {
        fData = SkData::MakeEmpty();
    }
    fOffset = 0;
}

std::unique_ptr<SkMemoryStream> SkMemoryStream::MakeCopy(const void* data, size_t length) {
    return std::make_unique<SkMemoryStream>(data, length, true);
}

std::unique_ptr<SkMemoryStream> SkMemoryStream::MakeDirect(const void* data, size_t length) {
    return std::make_unique<SkMemoryStream>(data, length, false);
}

std::unique_ptr<SkMemoryStream> SkMemoryStream::Make(sk_sp<const SkData> data) {
    return std::make_unique<SkMemoryStream>(std::move(data));
}

void SkMemoryStream::setMemoryOwned(const void* src, size_t size) {
    fData = SkData::MakeFromMalloc(src, size);
    fOffset = 0;
}

void SkMemoryStream::setMemory(const void* src, size_t size, bool copyData) {
    fData = newFromParams(src, size, copyData);
    fOffset = 0;
}

void SkMemoryStream::setData(sk_sp<const SkData> data) {
    if (nullptr == data) {
        fData = SkData::MakeEmpty();
    } else {
        fData = data;
    }
    fOffset = 0;
}

size_t SkMemoryStream::read(void* buffer, size_t size) {
    size_t dataSize = fData->size();
    SkASSERT(fOffset <= dataSize);
    size = skia_rs_stream_clamp_read(dataSize - fOffset, size);
    if (buffer) {
        sk_careful_memcpy(buffer, fData->bytes() + fOffset, size);
    }
    fOffset += size;
    return size;
}

size_t SkMemoryStream::peek(void* buffer, size_t size) const {
    SkASSERT(buffer != nullptr);
    const size_t currentOffset = fOffset;
    SkMemoryStream* nonConstThis = const_cast<SkMemoryStream*>(this);
    const size_t bytesRead = nonConstThis->read(buffer, size);
    nonConstThis->fOffset = currentOffset;
    return bytesRead;
}

bool SkMemoryStream::isAtEnd() const {
    return fOffset == fData->size();
}

bool SkMemoryStream::rewind() {
    fOffset = 0;
    return true;
}

SkMemoryStream* SkMemoryStream::onDuplicate() const {
    return new SkMemoryStream(fData);
}

size_t SkMemoryStream::getPosition() const {
    return fOffset;
}

bool SkMemoryStream::seek(size_t position) {
    fOffset = skia_rs_stream_memory_seek(fData->size(), position);
    return true;
}

bool SkMemoryStream::move(long offset) {
    fOffset = skia_rs_stream_memory_move(fData->size(), fOffset, static_cast<intptr_t>(offset));
    return true;
}

SkMemoryStream* SkMemoryStream::onFork() const {
    std::unique_ptr<SkMemoryStream> that(this->duplicate());
    that->seek(fOffset);
    return that.release();
}

size_t SkMemoryStream::getLength() const {
    return fData->size();
}

const void* SkMemoryStream::getMemoryBase() {
    return fData->data();
}

const void* SkMemoryStream::getAtPos() {
    return fData->bytes() + fOffset;
}

// ============================================================================
// SkFILEWStream
// ============================================================================

SkFILEWStream::SkFILEWStream(const char path[]) {
    fFILE = sk_fopen(path, kWrite_SkFILE_Flag);
}

SkFILEWStream::~SkFILEWStream() {
    if (fFILE) {
        sk_fclose(fFILE);
    }
}

size_t SkFILEWStream::bytesWritten() const {
    return sk_ftell(fFILE);
}

bool SkFILEWStream::write(const void* buffer, size_t size) {
    if (fFILE == nullptr) {
        return false;
    }
    if (sk_fwrite(buffer, size, fFILE) != size) {
        SkDEBUGCODE(SkDebugf("SkFILEWStream failed writing %zu bytes\n", size);)
        sk_fclose(fFILE);
        fFILE = nullptr;
        return false;
    }
    return true;
}

void SkFILEWStream::flush() {
    if (fFILE) {
        sk_fflush(fFILE);
    }
}

void SkFILEWStream::fsync() {
    flush();
    if (fFILE) {
        sk_fsync(fFILE);
    }
}

// ============================================================================
// SkDynamicMemoryWStream & SkBlockMemoryStream
// ============================================================================

static inline void sk_memcpy_4bytes(void* dst, const void* src, size_t size) {
    if (size == 4) {
        memcpy(dst, src, 4);
    } else {
        memcpy(dst, src, size);
    }
}

struct SkDynamicMemoryWStream::Block {
    Block* fNext;
    char*  fCurr;
    char*  fStop;

    const char* start() const { return (const char*)(this + 1); }
    char*       start() { return (char*)(this + 1); }
    size_t      avail() const { return fStop - fCurr; }
    size_t      written() const { return fCurr - this->start(); }

    void init(size_t size) {
        fNext = nullptr;
        fCurr = this->start();
        fStop = this->start() + size;
    }

    const void* append(const void* data, size_t size) {
        SkASSERT((size_t)(fStop - fCurr) >= size);
        sk_memcpy_4bytes(fCurr, data, size);
        fCurr += size;
        return (const void*)((const char*)data + size);
    }
};

SkDynamicMemoryWStream::SkDynamicMemoryWStream(SkDynamicMemoryWStream&& other)
    : fHead(other.fHead)
    , fTail(other.fTail)
    , fBytesWrittenBeforeTail(other.fBytesWrittenBeforeTail)
{
    other.fHead = nullptr;
    other.fTail = nullptr;
    other.fBytesWrittenBeforeTail = 0;
}

SkDynamicMemoryWStream& SkDynamicMemoryWStream::operator=(SkDynamicMemoryWStream&& other) {
    if (this != &other) {
        this->~SkDynamicMemoryWStream();
        new (this) SkDynamicMemoryWStream(std::move(other));
    }
    return *this;
}

SkDynamicMemoryWStream::~SkDynamicMemoryWStream() {
    this->reset();
}

void SkDynamicMemoryWStream::reset() {
    Block* block = fHead;
    while (block != nullptr) {
        Block* next = block->fNext;
        sk_free(block);
        block = next;
    }
    fHead = fTail = nullptr;
    fBytesWrittenBeforeTail = 0;
}

size_t SkDynamicMemoryWStream::bytesWritten() const {
    this->validate();
    if (fTail) {
        return fBytesWrittenBeforeTail + fTail->written();
    }
    return 0;
}

bool SkDynamicMemoryWStream::write(const void* buffer, size_t count) {
    if (count > 0) {
        SkASSERT(buffer);
        size_t size;

        if (fTail && fTail->avail() > 0) {
            size = std::min(fTail->avail(), count);
            buffer = fTail->append(buffer, size);
            SkASSERT(count >= size);
            count -= size;
            if (count == 0) {
                return true;
            }
        }

        size = skia_rs_stream_dynamic_block_payload_size(count, sizeof(Block));

        Block* block = (Block*)sk_malloc_canfail(sizeof(Block) + size);
        if (!block) {
            this->validate();
            return false;
        }
        block->init(size);
        block->append(buffer, count);

        if (fTail) {
            fBytesWrittenBeforeTail += fTail->written();
            fTail->fNext = block;
        } else {
            fHead = fTail = block;
        }
        fTail = block;
        this->validate();
    }
    return true;
}

bool SkDynamicMemoryWStream::writeToAndReset(SkDynamicMemoryWStream* dst) {
    SkASSERT(dst);
    SkASSERT(dst != this);
    if (0 == this->bytesWritten()) {
        return true;
    }
    if (0 == dst->bytesWritten()) {
        *dst = std::move(*this);
        return true;
    }
    dst->fTail->fNext = fHead;
    dst->fBytesWrittenBeforeTail += fBytesWrittenBeforeTail + dst->fTail->written();
    dst->fTail = fTail;
    fHead = fTail = nullptr;
    fBytesWrittenBeforeTail = 0;
    return true;
}

void SkDynamicMemoryWStream::prependToAndReset(SkDynamicMemoryWStream* dst) {
    SkASSERT(dst);
    SkASSERT(dst != this);
    if (0 == this->bytesWritten()) {
        return;
    }
    if (0 == dst->bytesWritten()) {
        *dst = std::move(*this);
        return;
    }
    fTail->fNext = dst->fHead;
    dst->fHead = fHead;
    dst->fBytesWrittenBeforeTail += fBytesWrittenBeforeTail + fTail->written();
    fHead = fTail = nullptr;
    fBytesWrittenBeforeTail = 0;
}

bool SkDynamicMemoryWStream::read(void* buffer, size_t offset, size_t count) {
    if (offset + count > this->bytesWritten()) {
        return false;
    }
    Block* block = fHead;
    while (block != nullptr) {
        size_t size = block->written();
        if (offset < size) {
            size_t part = offset + count > size ? size - offset : count;
            memcpy(buffer, block->start() + offset, part);
            if (count <= part) {
                return true;
            }
            count -= part;
            buffer = (void*)((char*)buffer + part);
        }
        offset = offset > size ? offset - size : 0;
        block = block->fNext;
    }
    return false;
}

void SkDynamicMemoryWStream::copyTo(void* dst) const {
    SkASSERT(dst);
    Block* block = fHead;
    while (block != nullptr) {
        size_t size = block->written();
        memcpy(dst, block->start(), size);
        dst = (void*)((char*)dst + size);
        block = block->fNext;
    }
}

bool SkDynamicMemoryWStream::writeToStream(SkWStream* dst) const {
    SkASSERT(dst);
    for (Block* block = fHead; block != nullptr; block = block->fNext) {
        if (!dst->write(block->start(), block->written())) {
            return false;
        }
    }
    return true;
}

void SkDynamicMemoryWStream::padToAlign4() {
    if (fTail) {
        size_t padBytes = skia_rs_stream_dynamic_pad_align4_bytes(fTail->written());
        if (padBytes) {
            int zero = 0;
            fTail->append(&zero, padBytes);
        }
    }
}

void SkDynamicMemoryWStream::copyToAndReset(void* ptr) {
    if (!ptr) {
        this->reset();
        return;
    }
    char* dst = reinterpret_cast<char*>(ptr);
    Block* block = fHead;
    while (block != nullptr) {
        size_t len = block->written();
        memcpy(dst, block->start(), len);
        dst += len;
        Block* next = block->fNext;
        sk_free(block);
        block = next;
    }
    fHead = fTail = nullptr;
    fBytesWrittenBeforeTail = 0;
}

bool SkDynamicMemoryWStream::writeToAndReset(SkWStream* dst) {
    SkASSERT(dst);
    bool dstStreamGood = true;
    for (Block* block = fHead; block != nullptr; ) {
        if (dstStreamGood && !dst->write(block->start(), block->written())) {
            dstStreamGood = false;
        }
        Block* next = block->fNext;
        sk_free(block);
        block = next;
    }
    fHead = fTail = nullptr;
    fBytesWrittenBeforeTail = 0;
    return dstStreamGood;
}

sk_sp<SkData> SkDynamicMemoryWStream::detachAsData() {
    const size_t size = this->bytesWritten();
    if (0 == size) {
        return SkData::MakeEmpty();
    }
    sk_sp<SkData> data = SkData::MakeUninitialized(size);
    this->copyToAndReset(data->writable_data());
    return data;
}

std::vector<uint8_t> SkDynamicMemoryWStream::detachAsVector() {
    std::vector<uint8_t> result;
    const size_t size = this->bytesWritten();
    if (0 == size) {
        return result;
    }
    result.resize(size);
    this->copyToAndReset(result.data());
    return result;
}

#ifdef SK_DEBUG
void SkDynamicMemoryWStream::validate() const {
    if (!fHead) {
        SkASSERT(!fTail);
        SkASSERT(fBytesWrittenBeforeTail == 0);
        return;
    }
    SkASSERT(fTail);

    size_t bytes = 0;
    const Block* block = fHead;
    while (block) {
        if (block->fNext) {
            bytes += block->written();
        }
        block = block->fNext;
    }
    SkASSERT(bytes == fBytesWrittenBeforeTail);
}
#endif

class SkBlockMemoryRefCnt : public SkRefCnt {
public:
    explicit SkBlockMemoryRefCnt(SkDynamicMemoryWStream::Block* head) : fHead(head) {}

    ~SkBlockMemoryRefCnt() override {
        SkDynamicMemoryWStream::Block* block = fHead;
        while (block != nullptr) {
            SkDynamicMemoryWStream::Block* next = block->fNext;
            sk_free(block);
            block = next;
        }
    }

    SkDynamicMemoryWStream::Block* const fHead;
};

class SkBlockMemoryStream : public SkStreamAsset {
public:
    SkBlockMemoryStream(sk_sp<SkBlockMemoryRefCnt> headRef, size_t size)
        : fBlockMemory(std::move(headRef))
        , fCurrent(fBlockMemory->fHead)
        , fSize(size)
        , fOffset(0)
        , fCurrentOffset(0) {}

    size_t read(void* buffer, size_t rawCount) override {
        size_t count = rawCount;
        if (fOffset + count > fSize) {
            count = fSize - fOffset;
        }
        size_t bytesLeftToRead = count;
        while (fCurrent != nullptr) {
            size_t bytesLeftInCurrent = fCurrent->written() - fCurrentOffset;
            size_t bytesFromCurrent = std::min(bytesLeftToRead, bytesLeftInCurrent);
            if (buffer) {
                memcpy(buffer, fCurrent->start() + fCurrentOffset, bytesFromCurrent);
                buffer = SkTAddOffset<void>(buffer, bytesFromCurrent);
            }
            if (bytesLeftToRead <= bytesFromCurrent) {
                fCurrentOffset += bytesFromCurrent;
                fOffset += count;
                return count;
            }
            bytesLeftToRead -= bytesFromCurrent;
            fCurrent = fCurrent->fNext;
            fCurrentOffset = 0;
        }
        SkASSERT(false);
        return 0;
    }

    bool isAtEnd() const override {
        return fOffset == fSize;
    }

    size_t peek(void* buff, size_t bytesToPeek) const override {
        SkASSERT(buff != nullptr);
        bytesToPeek = std::min(bytesToPeek, fSize - fOffset);
        size_t bytesLeftToPeek = bytesToPeek;
        char* buffer = static_cast<char*>(buff);
        const SkDynamicMemoryWStream::Block* current = fCurrent;
        size_t currentOffset = fCurrentOffset;
        while (bytesLeftToPeek) {
            SkASSERT(current);
            size_t bytesFromCurrent = std::min(current->written() - currentOffset, bytesLeftToPeek);
            memcpy(buffer, current->start() + currentOffset, bytesFromCurrent);
            bytesLeftToPeek -= bytesFromCurrent;
            buffer += bytesFromCurrent;
            current = current->fNext;
            currentOffset = 0;
        }
        return bytesToPeek;
    }

    bool rewind() override {
        fCurrent = fBlockMemory->fHead;
        fOffset = 0;
        fCurrentOffset = 0;
        return true;
    }

    SkBlockMemoryStream* onDuplicate() const override {
        return new SkBlockMemoryStream(fBlockMemory, fSize);
    }

    size_t getPosition() const override {
        return fOffset;
    }

    bool seek(size_t position) override {
        if (position >= fOffset) {
            size_t skipAmount = position - fOffset;
            return this->skip(skipAmount) == skipAmount;
        }
        size_t moveBackAmount = fOffset - position;
        if (moveBackAmount <= fCurrentOffset) {
            fCurrentOffset -= moveBackAmount;
            fOffset -= moveBackAmount;
            return true;
        }
        return this->rewind() && this->skip(position) == position;
    }

    bool move(long offset) override {
        return seek(fOffset + offset);
    }

    SkBlockMemoryStream* onFork() const override {
        SkBlockMemoryStream* that = this->onDuplicate();
        that->fCurrent = this->fCurrent;
        that->fOffset = this->fOffset;
        that->fCurrentOffset = this->fCurrentOffset;
        return that;
    }

    size_t getLength() const override {
        return fSize;
    }

    const void* getMemoryBase() override {
        if (fBlockMemory->fHead && !fBlockMemory->fHead->fNext) {
            return fBlockMemory->fHead->start();
        }
        return nullptr;
    }

private:
    sk_sp<SkBlockMemoryRefCnt> const fBlockMemory;
    SkDynamicMemoryWStream::Block const* fCurrent;
    size_t const fSize;
    size_t fOffset;
    size_t fCurrentOffset;
};

std::unique_ptr<SkStreamAsset> SkDynamicMemoryWStream::detachAsStream() {
    if (nullptr == fHead) {
        return SkMemoryStream::Make(nullptr);
    }
    if (fHead == fTail) {
        ptrdiff_t used = fTail->fCurr - (char*)fTail;
        fHead = fTail = (SkDynamicMemoryWStream::Block*)sk_realloc_throw(fTail, SkToSizeT(used));
        fTail->fStop = fTail->fCurr = (char*)fTail + used;
        SkASSERT(nullptr == fTail->fNext);
        SkASSERT(0 == fBytesWrittenBeforeTail);
    }
    std::unique_ptr<SkStreamAsset> stream = std::make_unique<SkBlockMemoryStream>(
            sk_make_sp<SkBlockMemoryRefCnt>(fHead), this->bytesWritten());
    fHead = nullptr;
    this->reset();
    return stream;
}

static sk_sp<SkData> mmap_filename(const char path[]) {
    FILE* file = sk_fopen(path, kRead_SkFILE_Flag);
    if (nullptr == file) {
        return nullptr;
    }
    auto data = SkData::MakeFromFILE(file);
    sk_fclose(file);
    return data;
}

std::unique_ptr<SkStreamAsset> SkStream::MakeFromFile(const char path[]) {
    auto data(mmap_filename(path));
    if (data) {
        return std::make_unique<SkMemoryStream>(std::move(data));
    }
    auto stream = std::make_unique<SkFILEStream>(path);
    if (!stream->isValid()) {
        return nullptr;
    }
    return stream;
}

namespace SkStreamPriv {

constexpr size_t kBufferSize = 4096;

sk_sp<SkData> CopyStreamToData(SkStream* stream) {
    SkASSERT(stream != nullptr);
    if (stream->hasLength()) {
        return SkData::MakeFromStream(stream, stream->getLength());
    }
    SkDynamicMemoryWStream tempStream;
    char buffer[kBufferSize];
    do {
        size_t bytesRead = stream->read(buffer, kBufferSize);
        tempStream.write(buffer, bytesRead);
    } while (!stream->isAtEnd());
    return tempStream.detachAsData();
}

bool Copy(SkWStream* out, SkStream* input) {
    const char* base = static_cast<const char*>(input->getMemoryBase());
    if (base && input->hasPosition() && input->hasLength()) {
        size_t position = input->getPosition();
        size_t length = input->getLength();
        SkASSERT(length >= position);
        return out->write(&base[position], length - position);
    }
    char scratch[kBufferSize];
    size_t count;
    while (true) {
        count = input->read(scratch, kBufferSize);
        if (0 == count) {
            return true;
        }
        if (!out->write(scratch, count)) {
            return false;
        }
    }
}

bool RemainingLengthIsBelow(SkStream* stream, size_t len) {
    SkASSERT(stream);
    bool hasLength = stream->hasLength();
    size_t length = hasLength ? stream->getLength() : 0;
    bool hasPosition = hasLength && stream->hasPosition();
    size_t position = hasPosition ? stream->getPosition() : 0;
    return skia_rs_stream_remaining_length_is_below(hasLength, length, hasPosition, position, len);
}

bool DebugfStream::write(const void* buffer, size_t size) {
    SkDebugf("%.*s", (int)size, (const char*)buffer);
    fBytesWritten += size;
    return true;
}

size_t DebugfStream::bytesWritten() const { return fBytesWritten; }

}  // namespace SkStreamPriv
