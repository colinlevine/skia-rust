/*
 * Safe Rust bridge for:
 *   src/core/SkDrawShadowInfo.cpp
 * (Phase 1 Layer 4.3 skia-rs-effects, GN target :core).
 */

#include "src/core/SkDrawShadowInfo.h"

#include "include/core/SkMatrix.h"
#include "include/core/SkPath.h"
#include "include/core/SkPoint3.h"
#include "include/core/SkRect.h"

static_assert(sizeof(SkMatrix) == 40);
static_assert(sizeof(SkPoint3) == 12);
static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkDrawShadowRec) == 40);

extern "C" {
bool skia_rs_shadow_get_spot_transform(SkPoint3 light_pos,
                                       float light_radius,
                                       const SkMatrix* ctm,
                                       SkPoint3 z_plane_params,
                                       SkRect path_bounds,
                                       bool directional,
                                       SkMatrix* out_transform,
                                       float* out_radius);
SkRect skia_rs_shadow_get_local_bounds(SkRect path_bounds,
                                       const SkDrawShadowRec* rec,
                                       const SkMatrix* ctm);
}  // extern "C"

namespace SkDrawShadowMetrics {

bool GetSpotShadowTransform(const SkPoint3& lightPos, SkScalar lightRadius,
                            const SkMatrix& ctm, const SkPoint3& zPlaneParams,
                            const SkRect& pathBounds, bool directional,
                            SkMatrix* shadowTransform, SkScalar* radius) {
    return skia_rs_shadow_get_spot_transform(lightPos,
                                             lightRadius,
                                             &ctm,
                                             zPlaneParams,
                                             pathBounds,
                                             directional,
                                             shadowTransform,
                                             radius);
}

void GetLocalBounds(const SkPath& path, const SkDrawShadowRec& rec, const SkMatrix& ctm,
                    SkRect* bounds) {
    *bounds = skia_rs_shadow_get_local_bounds(path.getBounds(), &rec, &ctm);
}

}  // namespace SkDrawShadowMetrics
