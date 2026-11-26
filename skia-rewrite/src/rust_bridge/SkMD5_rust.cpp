/*
 * Safe Rust bridge for SkMD5 (backed by skia-rs-math::md5 via skia-rs-ffi).
 */

#include "src/core/SkMD5.h"

#include "include/core/SkSpan.h"
#include "src/core/SkUtils.h"

#include <cstdint>
#include <cstring>

extern "C" {
struct SkiaRsMd5 {
    uint64_t byte_count;
    uint32_t state[4];
    uint8_t buffer[64];
};

void skia_rs_md5_init(SkiaRsMd5* out);
void skia_rs_md5_update(SkiaRsMd5* state, const uint8_t* data, size_t len);
void skia_rs_md5_finish(SkiaRsMd5* state, uint8_t digest_out[16]);
}

SkMD5::SkMD5() : byteCount(0) {
    SkiaRsMd5 raw;
    skia_rs_md5_init(&raw);
    this->byteCount = raw.byte_count;
    std::memcpy(this->state.data(), raw.state, sizeof(raw.state));
    std::memcpy(this->buffer.data(), raw.buffer, sizeof(raw.buffer));
}

bool SkMD5::write(const void* buf, size_t inputLength) {
    return this->write(
            SK_UNSAFE_TODO(SkSpan(static_cast<const uint8_t*>(buf), inputLength)));
}

bool SkMD5::write(SkSpan<const uint8_t> input) {
    SkiaRsMd5 raw;
    raw.byte_count = this->byteCount;
    std::memcpy(raw.state, this->state.data(), sizeof(raw.state));
    std::memcpy(raw.buffer, this->buffer.data(), sizeof(raw.buffer));

    skia_rs_md5_update(&raw, input.data(), input.size());

    this->byteCount = raw.byte_count;
    std::memcpy(this->state.data(), raw.state, sizeof(raw.state));
    std::memcpy(this->buffer.data(), raw.buffer, sizeof(raw.buffer));
    return true;
}

SkMD5::Digest SkMD5::finish() {
    SkiaRsMd5 raw;
    raw.byte_count = this->byteCount;
    std::memcpy(raw.state, this->state.data(), sizeof(raw.state));
    std::memcpy(raw.buffer, this->buffer.data(), sizeof(raw.buffer));

    SkMD5::Digest digest;
    skia_rs_md5_finish(&raw, digest.data.data());

    this->byteCount = raw.byte_count;
    std::memcpy(this->state.data(), raw.state, sizeof(raw.state));
    std::memcpy(this->buffer.data(), raw.buffer, sizeof(raw.buffer));
    return digest;
}

static SkString to_hex_string(SkSpan<const uint8_t, 16> data, SkSpan<const char, 16> hexDigits) {
    static_assert(data.size() == sizeof(SkMD5::Digest::data));
    SkString hexString(2 * data.size());
    for (size_t i = 0; i < data.size(); ++i) {
        uint8_t byte = data[i];
        hexString[2 * i + 0] = hexDigits[byte >> 4];
        hexString[2 * i + 1] = hexDigits[byte & 0xF];
    }
    return hexString;
}

SkString SkMD5::Digest::toHexString() const {
    return to_hex_string(data, SkHexadecimalDigits::gUpper);
}

SkString SkMD5::Digest::toLowercaseHexString() const {
    return to_hex_string(data, SkHexadecimalDigits::gLower);
}
