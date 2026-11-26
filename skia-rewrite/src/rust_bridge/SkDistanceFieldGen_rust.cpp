/*
 * Safe Rust bridge for src/core/SkDistanceFieldGen.cpp
 * (Phase 1 Layer 3.2: skia-rs-scan)
 */

#include "src/core/SkDistanceFieldGen.h"

#include "include/private/SkAssert.h"

#include <cstddef>
#include <cstdint>

#if !defined(SK_DISABLE_SDF_TEXT)

extern "C" {

size_t skia_rs_compute_distance_field_size(int32_t w, int32_t h);
bool skia_rs_generate_distance_field_a8(
        uint8_t* distance_field, const uint8_t* image, int32_t w, int32_t h, size_t row_bytes);
bool skia_rs_generate_distance_field_lcd16(
        uint8_t* distance_field, const uint8_t* image, int32_t w, int32_t h, size_t row_bytes);
bool skia_rs_generate_distance_field_bw(
        uint8_t* distance_field, const uint8_t* image, int32_t w, int32_t h, size_t row_bytes);

}  // extern "C"

size_t SkComputeDistanceFieldSize(int w, int h) {
    return skia_rs_compute_distance_field_size(w, h);
}

bool SkGenerateDistanceFieldFromA8Image(unsigned char* distanceField,
                                        const unsigned char* image,
                                        int width,
                                        int height,
                                        size_t rowBytes) {
    SkASSERT(distanceField);
    SkASSERT(image);
    return skia_rs_generate_distance_field_a8(distanceField, image, width, height, rowBytes);
}

bool SkGenerateDistanceFieldFromLCD16Mask(unsigned char* distanceField,
                                          const unsigned char* image,
                                          int w,
                                          int h,
                                          size_t rowBytes) {
    SkASSERT(distanceField);
    SkASSERT(image);
    return skia_rs_generate_distance_field_lcd16(distanceField, image, w, h, rowBytes);
}

bool SkGenerateDistanceFieldFromBWImage(unsigned char* distanceField,
                                        const unsigned char* image,
                                        int width,
                                        int height,
                                        size_t rowBytes) {
    SkASSERT(distanceField);
    SkASSERT(image);
    return skia_rs_generate_distance_field_bw(distanceField, image, width, height, rowBytes);
}

#endif  // !defined(SK_DISABLE_SDF_TEXT)
