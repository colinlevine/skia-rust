/*
 * Safe Rust bridge for:
 *   src/effects/SkBlenders.cpp
 * (Phase 1 Layer 4.2 skia-rs-colorfilters, GN target :skia).
 */

#include "include/core/SkBlendMode.h"
#include "include/core/SkBlender.h"
#include "include/core/SkData.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkTypes.h"
#include "include/effects/SkBlenders.h"
#include "include/effects/SkRuntimeEffect.h"
#include "src/core/SkKnownRuntimeEffects.h"

#include <cstdint>

namespace {
struct SkiaRsArithmeticResult {
    int32_t kind;
    int32_t blend_mode;
    float uniforms[5];
};
}  // namespace

static_assert(sizeof(SkiaRsArithmeticResult) == 28);

extern "C" {
SkiaRsArithmeticResult skia_rs_blender_classify_arithmetic(
        float k1, float k2, float k3, float k4, bool enforce_premul);
}  // extern "C"

sk_sp<SkBlender> SkBlenders::Arithmetic(float k1, float k2, float k3, float k4,
                                        bool enforcePremul) {
    using namespace SkKnownRuntimeEffects;

    SkiaRsArithmeticResult res =
            skia_rs_blender_classify_arithmetic(k1, k2, k3, k4, enforcePremul);
    if (res.kind == 0) {
        return nullptr;
    }
    if (res.kind == 1) {
        return SkBlender::Mode(static_cast<SkBlendMode>(res.blend_mode));
    }

    const SkRuntimeEffect* arithmeticEffect = GetKnownRuntimeEffect(StableKey::kArithmetic);
    return arithmeticEffect->makeBlender(
            SkData::MakeWithCopy(res.uniforms, sizeof(res.uniforms)));
}
