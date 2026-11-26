/*
 * Safe Rust bridge for src/utils/SkMatrix22.cpp (Layer 1.2 skia-rs-geometry).
 */

#include "src/utils/SkMatrix22.h"

#include "include/core/SkMatrix.h"
#include "include/core/SkPoint.h"

extern "C" {
void skia_rs_compute_givens_rotation(SkPoint h, float* sin_out, float* cos_out);
}

void SkComputeGivensRotation(const SkVector& h, SkMatrix* G) {
    float s = 0.0f;
    float c = 0.0f;
    skia_rs_compute_givens_rotation(h, &s, &c);
    G->setSinCos(s, c);
}
