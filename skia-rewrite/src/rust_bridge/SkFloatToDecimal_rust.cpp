/*
 * Safe Rust bridge for SkFloatToDecimal (backed by skia-rs-math::float_to_decimal).
 */

#include "src/utils/SkFloatToDecimal.h"

#include <cstdint>

extern "C" {
uint32_t skia_rs_float_to_decimal(float value, uint8_t output[49]);
}

unsigned SkFloatToDecimal(float value, char output[kMaximumSkFloatToDecimalLength]) {
    static_assert(kMaximumSkFloatToDecimalLength == 49, "");
    return skia_rs_float_to_decimal(value, reinterpret_cast<uint8_t*>(output));
}
