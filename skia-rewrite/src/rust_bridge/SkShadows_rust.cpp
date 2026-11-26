/*
 * Safe Rust bridge for:
 *   src/utils/SkShadowTessellator.cpp
 *   src/utils/SkShadowUtils.cpp
 * (Phase 1 Layer 4.3 skia-rs-effects, GN target :skia).
 */

#include "include/core/SkBlendMode.h"
#include "include/core/SkBlender.h"
#include "include/core/SkBlurTypes.h"
#include "include/core/SkCanvas.h"
#include "include/core/SkColor.h"
#include "include/core/SkColorFilter.h"
#include "include/core/SkM44.h"
#include "include/core/SkMaskFilter.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPath.h"
#include "include/core/SkPoint.h"
#include "include/core/SkPoint3.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkTypes.h"
#include "include/core/SkVertices.h"
#include "include/private/SkFloatingPoint.h"
#include "include/private/SkIDChangeListener.h"
#include "include/private/SkOnlyStackAllocatable.h"
#include "include/private/SkTDArray.h"
#include "include/private/SkTemplates.h"
#include "include/private/SkTo.h"
#include "include/utils/SkShadowUtils.h"
#include "src/core/SkBlurMask.h"
#include "src/core/SkColorFilterPriv.h"
#include "src/core/SkDevice.h"
#include "src/core/SkDrawShadowInfo.h"
#include "src/core/SkGeometry.h"
#include "src/core/SkPathPriv.h"
#include "src/core/SkRandom.h"
#include "src/core/SkResourceCache.h"
#include "src/core/SkVerticesPriv.h"
#include "src/utils/SkPolyUtils.h"

#if !defined(SK_ENABLE_OPTIMIZE_SIZE)
#include "src/utils/SkShadowTessellator.h"
#endif

#if defined(SK_GANESH)
#include "src/gpu/ganesh/GrStyle.h"
#include "src/gpu/ganesh/geometry/GrPathUtils.h"
#include "src/gpu/ganesh/geometry/GrStyledShape.h"
#endif

#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <functional>
#include <memory>
#include <new>
#include <utility>

using namespace skia_private;

class SkRRect;

namespace {

struct RsShadowPolygonState;
struct RsShadowMesh;

struct RsSpotFactoryParams {
    SkPoint local_center;
    SkVector offset;
    int32_t occluder_type;
};

static_assert(sizeof(SkMatrix) == 40);
static_assert(sizeof(SkPoint) == 8);
static_assert(sizeof(SkPoint3) == 12);
static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkDrawShadowRec) == 40);
static_assert(sizeof(RsSpotFactoryParams) == 20);

}  // namespace

extern "C" {
bool skia_rs_shadow_validate_ambient_args(SkRect bounds, const SkMatrix* ctm, SkPoint3 z_plane);
bool skia_rs_shadow_validate_spot_args(SkRect bounds,
                                       const SkMatrix* ctm,
                                       SkPoint3 z_plane,
                                       SkPoint3 light_pos,
                                       float light_radius);
void skia_rs_shadow_ambient_inset_outset(SkPoint3 z_plane,
                                         SkRect bounds,
                                         float* out_inset,
                                         float* out_outset);
SkPoint skia_rs_shadow_spot_clip_quad_mid(SkPoint p0, SkPoint p1, SkPoint p2);
SkPoint skia_rs_shadow_spot_clip_conic_mid(SkPoint p0, SkPoint p1, SkPoint p2, float w);
void skia_rs_shadow_spot_clip_cubic_mids(SkPoint p0,
                                         SkPoint p1,
                                         SkPoint p2,
                                         SkPoint p3,
                                         SkPoint* out_mids);

RsShadowPolygonState* skia_rs_shadow_poly_state_new();
void skia_rs_shadow_poly_state_handle_line(RsShadowPolygonState* state, SkPoint p);
void skia_rs_shadow_poly_state_add_to_clip(RsShadowPolygonState* state, SkPoint p);
void skia_rs_shadow_poly_state_finish(RsShadowPolygonState* state);
void skia_rs_shadow_poly_state_info(const RsShadowPolygonState* state,
                                    const SkPoint** out_path_pts,
                                    size_t* out_path_len,
                                    const SkPoint** out_clip_pts,
                                    size_t* out_clip_len,
                                    SkPoint* out_centroid,
                                    float* out_area,
                                    bool* out_is_convex,
                                    float* out_direction);
void skia_rs_shadow_poly_state_free(RsShadowPolygonState* state);

void skia_rs_shadow_convex_prepare_inset(const SkPoint* path_pts,
                                         size_t path_len,
                                         SkPoint centroid,
                                         float inset,
                                         float* out_inset,
                                         uint32_t* out_umbra_color);
RsShadowMesh* skia_rs_shadow_build_convex_mesh(const SkPoint* path_pts,
                                               size_t path_len,
                                               const SkPoint* umbra_pts,
                                               size_t umbra_len,
                                               bool valid_umbra,
                                               const SkPoint* clip_pts,
                                               size_t clip_len,
                                               SkPoint centroid,
                                               float direction,
                                               float outset,
                                               uint32_t umbra_color,
                                               bool transparent,
                                               bool do_clip);
float skia_rs_shadow_concave_clamp_inset(float inset, SkRect path_bounds);
RsShadowMesh* skia_rs_shadow_stitch_concave_rings(size_t path_poly_len,
                                                  const SkPoint* umbra_pts,
                                                  size_t umbra_len,
                                                  int32_t* umbra_indices,
                                                  const SkPoint* penumbra_pts,
                                                  size_t penumbra_len,
                                                  int32_t* penumbra_indices);
void skia_rs_shadow_mesh_extract(const RsShadowMesh* mesh,
                                 const SkPoint** out_pos,
                                 const uint32_t** out_colors,
                                 size_t* out_vert_count,
                                 const uint16_t** out_indices,
                                 size_t* out_index_count,
                                 const uint16_t** out_index_map,
                                 size_t* out_index_map_len);
void skia_rs_shadow_mesh_free(RsShadowMesh* mesh);

void skia_rs_shadow_compute_tonal_colors(uint32_t in_ambient,
                                         uint32_t in_spot,
                                         uint32_t* out_ambient,
                                         uint32_t* out_spot);
bool skia_rs_shadow_fill_rec(SkPoint3 z_plane,
                             SkPoint3 light_pos,
                             float light_radius,
                             uint32_t ambient_color,
                             uint32_t spot_color,
                             uint32_t flags,
                             const SkMatrix* ctm,
                             SkDrawShadowRec* out_rec);
bool skia_rs_shadow_validate_rec(const SkDrawShadowRec* rec);
bool skia_rs_shadow_is_z_plane_tilted(SkPoint3 z_plane);
void skia_rs_shadow_ambient_blur_fallback_params(float z_plane_z,
                                                 float* out_blur_radius,
                                                 float* out_stroke_width);
RsSpotFactoryParams skia_rs_shadow_compute_spot_factory_params(SkRect path_bounds,
                                                               bool path_is_convex,
                                                               const SkMatrix* view_matrix,
                                                               float z_plane_z,
                                                               SkPoint3 dev_light_pos,
                                                               float light_radius,
                                                               bool transparent,
                                                               bool directional);
}  // extern "C"

// ============================================================================
// SkShadowTessellator (src/utils/SkShadowTessellator.cpp)
// ============================================================================

#if !defined(SK_ENABLE_OPTIMIZE_SIZE)

namespace {

#if defined(SK_GANESH)
static constexpr SkScalar kQuadTolerance = 0.2f;
static constexpr SkScalar kCubicTolerance = 0.2f;
static constexpr SkScalar kQuadToleranceSqd = kQuadTolerance * kQuadTolerance;
static constexpr SkScalar kCubicToleranceSqd = kCubicTolerance * kCubicTolerance;
#endif
static constexpr SkScalar kConicTolerance = 0.25f;

static inline void spancpy(SkSpan<SkPoint> dst, SkSpan<const SkPoint> src) {
    SkASSERT(dst.size() >= src.size());
    for (size_t i = 0; i < src.size(); ++i) {
        dst[i] = src[i];
    }
}

class RustShadowTessellatorHelper {
public:
    RustShadowTessellatorHelper() : fState(skia_rs_shadow_poly_state_new()) {}
    ~RustShadowTessellatorHelper() { skia_rs_shadow_poly_state_free(fState); }

    RsShadowPolygonState* state() const { return fState; }

    void handleLine(const SkPoint& p) {
        skia_rs_shadow_poly_state_handle_line(fState, p);
    }

    void handleLine(const SkMatrix& m, SkPoint* p) {
        *p = m.mapPoint(*p);
        this->handleLine(*p);
    }

    void handleQuad(const SkPoint pts[3]) {
#if defined(SK_GANESH)
        SkVector v0 = pts[1] - pts[0];
        SkVector v1 = pts[2] - pts[0];
        if (SkScalarNearlyZero(v0.cross(v1))) {
            return;
        }
        int maxCount = GrPathUtils::quadraticPointCount(pts, kQuadTolerance);
        fPointBuffer.resize(maxCount);
        SkPoint* target = fPointBuffer.begin();
        int count = GrPathUtils::generateQuadraticPoints(
                pts[0], pts[1], pts[2], kQuadToleranceSqd, &target, maxCount);
        fPointBuffer.resize(count);
        for (int i = 0; i < count; ++i) {
            this->handleLine(fPointBuffer[i]);
        }
#else
        this->handleLine(pts[1]);
        this->handleLine(pts[2]);
#endif
    }

    void handleQuad(const SkMatrix& m, SkPoint pts[3]) {
        m.mapPoints({pts, 3});
        this->handleQuad(pts);
    }

    void handleCubic(const SkMatrix& m, SkPoint pts[4]) {
        m.mapPoints({pts, 4});
#if defined(SK_GANESH)
        int maxCount = GrPathUtils::cubicPointCount(pts, kCubicTolerance);
        fPointBuffer.resize(maxCount);
        SkPoint* target = fPointBuffer.begin();
        int count = GrPathUtils::generateCubicPoints(
                pts[0], pts[1], pts[2], pts[3], kCubicToleranceSqd, &target, maxCount);
        fPointBuffer.resize(count);
        for (int i = 0; i < count; ++i) {
            this->handleLine(fPointBuffer[i]);
        }
#else
        this->handleLine(pts[1]);
        this->handleLine(pts[2]);
        this->handleLine(pts[3]);
#endif
    }

    void handleConic(const SkMatrix& m, SkPoint pts[3], SkScalar w) {
        if (m.hasPerspective()) {
            w = SkConic::TransformW(pts, w, m);
        }
        m.mapPoints({pts, 3});
        SkAutoConicToQuads quadder;
        const SkPoint* quads = quadder.computeQuads(pts, w, kConicTolerance);
        SkPoint lastPoint = *(quads++);
        int count = quadder.countQuads();
        for (int i = 0; i < count; ++i) {
            SkPoint quadPts[3];
            quadPts[0] = lastPoint;
            quadPts[1] = quads[0];
            quadPts[2] = (i == count - 1) ? pts[2] : quads[1];
            this->handleQuad(quadPts);
            lastPoint = quadPts[2];
            quads += 2;
        }
    }

    sk_sp<SkVertices> buildVertices(const SkRect& pathBounds,
                                    float inset,
                                    float outset,
                                    bool transparent,
                                    bool doClip) {
        const SkPoint* pathPts = nullptr;
        size_t pathLen = 0;
        const SkPoint* clipPts = nullptr;
        size_t clipLen = 0;
        SkPoint centroid = {0, 0};
        float area = 0.0f;
        bool isConvex = true;
        float direction = 1.0f;
        skia_rs_shadow_poly_state_info(fState, &pathPts, &pathLen, &clipPts, &clipLen,
                                       &centroid, &area, &isConvex, &direction);

        if ((doClip && clipLen < 3) || pathLen < 3 || !SkIsFinite(area)) {
            return SkVertices::MakeCopy(SkVertices::kTriangles_VertexMode, 0,
                                        nullptr, nullptr, nullptr, 0, nullptr);
        }

        if (isConvex) {
            float adjInset = inset;
            uint32_t umbraColor = SK_ColorBLACK;
            skia_rs_shadow_convex_prepare_inset(
                    pathPts, pathLen, centroid, inset, &adjInset, &umbraColor);

            SkTDArray<SkPoint> insetPolygon;
            bool validUmbra = true;
            if (adjInset > SK_ScalarNearlyZero) {
                if (!SkInsetConvexPolygon(pathPts, static_cast<int>(pathLen), adjInset,
                                          &insetPolygon)) {
                    validUmbra = false;
                }
            }
            const SkPoint* umbraPtr =
                    (adjInset > SK_ScalarNearlyZero) ? insetPolygon.begin() : pathPts;
            const size_t umbraLen = (adjInset > SK_ScalarNearlyZero)
                                            ? static_cast<size_t>(insetPolygon.size())
                                            : pathLen;

            RsShadowMesh* mesh = skia_rs_shadow_build_convex_mesh(
                    pathPts, pathLen, umbraPtr, umbraLen, validUmbra,
                    clipPts, clipLen, centroid, direction, outset, umbraColor,
                    transparent, doClip);
            if (!mesh) {
                return nullptr;
            }
            const SkPoint* pos = nullptr;
            const uint32_t* colors = nullptr;
            size_t vertCount = 0;
            const uint16_t* indices = nullptr;
            size_t indexCount = 0;
            skia_rs_shadow_mesh_extract(mesh, &pos, &colors, &vertCount,
                                        &indices, &indexCount, nullptr, nullptr);
            sk_sp<SkVertices> result = SkVertices::MakeCopy(
                    SkVertices::kTriangles_VertexMode,
                    static_cast<int>(vertCount),
                    pos,
                    nullptr,
                    colors,
                    static_cast<int>(indexCount),
                    indices);
            skia_rs_shadow_mesh_free(mesh);
            return result;
        }

        // Concave shadow
        if (!SkIsSimplePolygon(pathPts, static_cast<int>(pathLen))) {
            return nullptr;
        }
        float clampedInset = skia_rs_shadow_concave_clamp_inset(inset, pathBounds);

        SkTDArray<SkPoint> umbraPolygon;
        SkTDArray<int> umbraIndices;
        umbraIndices.reserve(static_cast<int>(pathLen));
        if (!SkOffsetSimplePolygon(pathPts, static_cast<int>(pathLen), pathBounds, clampedInset,
                                   &umbraPolygon, &umbraIndices)) {
            return nullptr;
        }

        SkTDArray<SkPoint> penumbraPolygon;
        SkTDArray<int> penumbraIndices;
        penumbraPolygon.reserve(umbraPolygon.size());
        penumbraIndices.reserve(umbraPolygon.size());
        if (!SkOffsetSimplePolygon(pathPts, static_cast<int>(pathLen), pathBounds, -outset,
                                   &penumbraPolygon, &penumbraIndices)) {
            return nullptr;
        }

        if (umbraPolygon.empty() || penumbraPolygon.empty()) {
            return nullptr;
        }

        RsShadowMesh* mesh = skia_rs_shadow_stitch_concave_rings(
                pathLen,
                umbraPolygon.begin(),
                static_cast<size_t>(umbraPolygon.size()),
                umbraIndices.begin(),
                penumbraPolygon.begin(),
                static_cast<size_t>(penumbraPolygon.size()),
                penumbraIndices.begin());
        if (!mesh) {
            return nullptr;
        }

        const SkPoint* pos = nullptr;
        const uint32_t* colors = nullptr;
        size_t vertCount = 0;
        const uint16_t* indices = nullptr;
        size_t indexCount = 0;
        const uint16_t* indexMap = nullptr;
        size_t indexMapLen = 0;
        skia_rs_shadow_mesh_extract(mesh, &pos, &colors, &vertCount,
                                    &indices, &indexCount, &indexMap, &indexMapLen);

        sk_sp<SkVertices> result;
        if (transparent) {
            SkTDArray<uint16_t> allIndices;
            allIndices.reserve(static_cast<int>(indexCount) + 3 * umbraPolygon.size());
            allIndices.append(static_cast<int>(indexCount), indices);
            AutoSTMalloc<64, uint16_t> mutableIndexMap(indexMapLen);
            if (indexMapLen > 0) {
                memcpy(mutableIndexMap.get(), indexMap, indexMapLen * sizeof(uint16_t));
            }
            SkTriangulateSimplePolygon(umbraPolygon.begin(), mutableIndexMap.get(),
                                       umbraPolygon.size(), &allIndices);
            result = SkVertices::MakeCopy(SkVertices::kTriangles_VertexMode,
                                          static_cast<int>(vertCount),
                                          pos,
                                          nullptr,
                                          colors,
                                          allIndices.size(),
                                          allIndices.begin());
        } else {
            result = SkVertices::MakeCopy(SkVertices::kTriangles_VertexMode,
                                          static_cast<int>(vertCount),
                                          pos,
                                          nullptr,
                                          colors,
                                          static_cast<int>(indexCount),
                                          indices);
        }
        skia_rs_shadow_mesh_free(mesh);
        return result;
    }

private:
    RsShadowPolygonState* fState;
    SkTDArray<SkPoint> fPointBuffer;
};

}  // namespace

sk_sp<SkVertices> SkShadowTessellator::MakeAmbient(const SkPath& path, const SkMatrix& ctm,
                                                   const SkPoint3& zPlane, bool transparent) {
    const SkRect bounds = path.getBounds();
    if (!skia_rs_shadow_validate_ambient_args(bounds, &ctm, zPlane)) {
        return nullptr;
    }

    float inset = 0.0f;
    float outset = 0.0f;
    skia_rs_shadow_ambient_inset_outset(zPlane, bounds, &inset, &outset);

    RustShadowTessellatorHelper helper;
    SkPath::Iter iter(path, true);
    bool verbSeen = false;
    bool closeSeen = false;
    while (auto rec = iter.next()) {
        if (closeSeen) {
            return nullptr;
        }
        SkPoint pts[4];
        spancpy(pts, rec->fPoints);
        switch (rec->fVerb) {
            case SkPathVerb::kLine:
                helper.handleLine(ctm, &pts[1]);
                break;
            case SkPathVerb::kQuad:
                helper.handleQuad(ctm, pts);
                break;
            case SkPathVerb::kCubic:
                helper.handleCubic(ctm, pts);
                break;
            case SkPathVerb::kConic:
                helper.handleConic(ctm, pts, rec->conicWeight());
                break;
            case SkPathVerb::kMove:
                if (verbSeen) {
                    return nullptr;
                }
                break;
            case SkPathVerb::kClose:
                closeSeen = true;
                break;
        }
        verbSeen = true;
    }
    skia_rs_shadow_poly_state_finish(helper.state());
    return helper.buildVertices(bounds, inset, outset, transparent, /*doClip=*/false);
}

sk_sp<SkVertices> SkShadowTessellator::MakeSpot(const SkPath& path, const SkMatrix& ctm,
                                                const SkPoint3& zPlane, const SkPoint3& lightPos,
                                                SkScalar lightRadius, bool transparent,
                                                bool directional) {
    const SkRect bounds = path.getBounds();
    if (!skia_rs_shadow_validate_spot_args(bounds, &ctm, zPlane, lightPos, lightRadius)) {
        return nullptr;
    }

    SkMatrix shadowTransform;
    SkScalar outset;
    if (!SkDrawShadowMetrics::GetSpotShadowTransform(lightPos, lightRadius, ctm, zPlane,
                                                     bounds, directional,
                                                     &shadowTransform, &outset)) {
        return nullptr;
    }
    SkScalar inset = outset;

    RustShadowTessellatorHelper helper;
    SkPath::Iter iter(path, true);
    SkPoint clipPts[4];
    bool closeSeen = false;
    bool verbSeen = false;
    while (auto rec = iter.next()) {
        if (closeSeen) {
            return nullptr;
        }
        SkPoint pts[4];
        spancpy(pts, rec->fPoints);
        switch (rec->fVerb) {
            case SkPathVerb::kLine:
                clipPts[0] = ctm.mapPoint(pts[1]);
                skia_rs_shadow_poly_state_add_to_clip(helper.state(), clipPts[0]);
                helper.handleLine(shadowTransform, &pts[1]);
                break;
            case SkPathVerb::kQuad:
                ctm.mapPoints({clipPts, 3}, {pts, 3});
                skia_rs_shadow_poly_state_add_to_clip(
                        helper.state(),
                        skia_rs_shadow_spot_clip_quad_mid(clipPts[0], clipPts[1], clipPts[2]));
                skia_rs_shadow_poly_state_add_to_clip(helper.state(), clipPts[2]);
                helper.handleQuad(shadowTransform, pts);
                break;
            case SkPathVerb::kConic: {
                ctm.mapPoints({clipPts, 3}, {pts, 3});
                const float w = rec->conicWeight();
                skia_rs_shadow_poly_state_add_to_clip(
                        helper.state(),
                        skia_rs_shadow_spot_clip_conic_mid(clipPts[0], clipPts[1], clipPts[2], w));
                skia_rs_shadow_poly_state_add_to_clip(helper.state(), clipPts[2]);
                helper.handleConic(shadowTransform, pts, w);
            } break;
            case SkPathVerb::kCubic: {
                ctm.mapPoints({clipPts, 4}, {pts, 4});
                SkPoint mids[2];
                skia_rs_shadow_spot_clip_cubic_mids(
                        clipPts[0], clipPts[1], clipPts[2], clipPts[3], mids);
                skia_rs_shadow_poly_state_add_to_clip(helper.state(), mids[0]);
                skia_rs_shadow_poly_state_add_to_clip(helper.state(), mids[1]);
                skia_rs_shadow_poly_state_add_to_clip(helper.state(), clipPts[3]);
                helper.handleCubic(shadowTransform, pts);
            } break;
            case SkPathVerb::kMove:
                if (verbSeen) {
                    return nullptr;
                }
                break;
            case SkPathVerb::kClose:
                closeSeen = true;
                break;
        }
        verbSeen = true;
    }
    skia_rs_shadow_poly_state_finish(helper.state());
    return helper.buildVertices(bounds, inset, outset, transparent, /*doClip=*/true);
}

#endif  // !defined(SK_ENABLE_OPTIMIZE_SIZE)

// ============================================================================
// SkShadowUtils (src/utils/SkShadowUtils.cpp)
// ============================================================================

#if !defined(SK_ENABLE_OPTIMIZE_SIZE)
namespace {

uint64_t resource_cache_shared_id() {
    return 0x2020776f64616873llu;
}

struct AmbientVerticesFactory {
    SkScalar fOccluderHeight = SK_ScalarNaN;
    bool fTransparent;
    SkVector fOffset;

    bool isCompatible(const AmbientVerticesFactory& that, SkVector* translate) const {
        if (fOccluderHeight != that.fOccluderHeight || fTransparent != that.fTransparent) {
            return false;
        }
        *translate = that.fOffset;
        return true;
    }

    sk_sp<SkVertices> makeVertices(const SkPath& path, const SkMatrix& ctm,
                                   SkVector* translate) const {
        SkPoint3 zParams = SkPoint3::Make(0, 0, fOccluderHeight);
        SkMatrix noTrans(ctm);
        if (!ctm.hasPerspective()) {
            noTrans[SkMatrix::kMTransX] = 0;
            noTrans[SkMatrix::kMTransY] = 0;
        }
        *translate = fOffset;
        return SkShadowTessellator::MakeAmbient(path, noTrans, zParams, fTransparent);
    }
};

struct SpotVerticesFactory {
    enum class OccluderType {
        kPointTransparent = 0,
        kPointOpaquePartialUmbra = 1,
        kPointOpaqueNoUmbra = 2,
        kDirectional = 3,
        kDirectionalTransparent = 4,
    };

    SkVector fOffset;
    SkPoint fLocalCenter;
    SkScalar fOccluderHeight = SK_ScalarNaN;
    SkPoint3 fDevLightPos;
    SkScalar fLightRadius;
    OccluderType fOccluderType;

    bool isCompatible(const SpotVerticesFactory& that, SkVector* translate) const {
        if (fOccluderHeight != that.fOccluderHeight || fDevLightPos.fZ != that.fDevLightPos.fZ ||
            fLightRadius != that.fLightRadius || fOccluderType != that.fOccluderType) {
            return false;
        }
        switch (fOccluderType) {
            case OccluderType::kPointTransparent:
            case OccluderType::kPointOpaqueNoUmbra:
                *translate = that.fOffset;
                return true;
            case OccluderType::kPointOpaquePartialUmbra:
                if (fOffset == that.fOffset) {
                    translate->set(0, 0);
                    return true;
                }
                return false;
            case OccluderType::kDirectional:
            case OccluderType::kDirectionalTransparent:
                *translate = that.fOffset - fOffset;
                return true;
        }
        SK_ABORT("Uninitialized occluder type?");
    }

    sk_sp<SkVertices> makeVertices(const SkPath& path, const SkMatrix& ctm,
                                   SkVector* translate) const {
        bool transparent = fOccluderType == OccluderType::kPointTransparent ||
                           fOccluderType == OccluderType::kDirectionalTransparent;
        bool directional = fOccluderType == OccluderType::kDirectional ||
                           fOccluderType == OccluderType::kDirectionalTransparent;
        SkPoint3 zParams = SkPoint3::Make(0, 0, fOccluderHeight);
        if (directional) {
            translate->set(0, 0);
            return SkShadowTessellator::MakeSpot(path, ctm, zParams, fDevLightPos, fLightRadius,
                                                 transparent, true);
        } else if (ctm.hasPerspective() ||
                   OccluderType::kPointOpaquePartialUmbra == fOccluderType) {
            translate->set(0, 0);
            return SkShadowTessellator::MakeSpot(path, ctm, zParams, fDevLightPos, fLightRadius,
                                                 transparent, false);
        } else {
            SkMatrix noTrans(ctm);
            noTrans[SkMatrix::kMTransX] = 0;
            noTrans[SkMatrix::kMTransY] = 0;
            SkPoint devCenter = noTrans.mapPoint(fLocalCenter);
            SkPoint3 centerLightPos = SkPoint3::Make(devCenter.fX, devCenter.fY, fDevLightPos.fZ);
            *translate = fOffset;
            return SkShadowTessellator::MakeSpot(path, noTrans, zParams,
                                                 centerLightPos, fLightRadius, transparent, false);
        }
    }
};

class CachedTessellations : public SkRefCnt {
public:
    size_t size() const { return fAmbientSet.size() + fSpotSet.size(); }

    sk_sp<SkVertices> find(const AmbientVerticesFactory& ambient, const SkMatrix& matrix,
                           SkVector* translate) const {
        return fAmbientSet.find(ambient, matrix, translate);
    }

    sk_sp<SkVertices> add(const SkPath& devPath, const AmbientVerticesFactory& ambient,
                          const SkMatrix& matrix, SkVector* translate) {
        return fAmbientSet.add(devPath, ambient, matrix, translate);
    }

    sk_sp<SkVertices> find(const SpotVerticesFactory& spot, const SkMatrix& matrix,
                           SkVector* translate) const {
        return fSpotSet.find(spot, matrix, translate);
    }

    sk_sp<SkVertices> add(const SkPath& devPath, const SpotVerticesFactory& spot,
                          const SkMatrix& matrix, SkVector* translate) {
        return fSpotSet.add(devPath, spot, matrix, translate);
    }

private:
    template <typename FACTORY, int MAX_ENTRIES>
    class Set {
    public:
        size_t size() const { return fSize; }

        sk_sp<SkVertices> find(const FACTORY& factory, const SkMatrix& matrix,
                               SkVector* translate) const {
            for (int i = 0; i < MAX_ENTRIES; ++i) {
                if (fEntries[i].fFactory.isCompatible(factory, translate)) {
                    const SkMatrix& m = fEntries[i].fMatrix;
                    if (matrix.hasPerspective() || m.hasPerspective()) {
                        if (matrix != fEntries[i].fMatrix) {
                            continue;
                        }
                    } else if (matrix.getScaleX() != m.getScaleX() ||
                               matrix.getSkewX() != m.getSkewX() ||
                               matrix.getScaleY() != m.getScaleY() ||
                               matrix.getSkewY() != m.getSkewY()) {
                        continue;
                    }
                    return fEntries[i].fVertices;
                }
            }
            return nullptr;
        }

        sk_sp<SkVertices> add(const SkPath& path, const FACTORY& factory, const SkMatrix& matrix,
                              SkVector* translate) {
            sk_sp<SkVertices> vertices = factory.makeVertices(path, matrix, translate);
            if (!vertices) {
                return nullptr;
            }
            int i;
            if (fCount < MAX_ENTRIES) {
                i = fCount++;
            } else {
                i = fRandom.nextULessThan(MAX_ENTRIES);
                fSize -= fEntries[i].fVertices->approximateSize();
            }
            fEntries[i].fFactory = factory;
            fEntries[i].fVertices = vertices;
            fEntries[i].fMatrix = matrix;
            fSize += vertices->approximateSize();
            return vertices;
        }

    private:
        struct Entry {
            FACTORY fFactory;
            sk_sp<SkVertices> fVertices;
            SkMatrix fMatrix;
        };
        std::array<Entry, MAX_ENTRIES> fEntries;
        int fCount = 0;
        size_t fSize = 0;
        SkRandom fRandom;
    };

    Set<AmbientVerticesFactory, 4> fAmbientSet;
    Set<SpotVerticesFactory, 4> fSpotSet;
};

class CachedTessellationsRec : public SkResourceCache::Rec {
public:
    CachedTessellationsRec(const SkResourceCache::Key& key,
                           sk_sp<CachedTessellations> tessellations)
        : fTessellations(std::move(tessellations)) {
        fKey.reset(new uint8_t[key.size()]);
        memcpy(fKey.get(), &key, key.size());
    }

    const Key& getKey() const override {
        return *reinterpret_cast<SkResourceCache::Key*>(fKey.get());
    }

    size_t bytesUsed() const override { return fTessellations->size(); }
    const char* getCategory() const override { return "tessellated shadow masks"; }
    sk_sp<CachedTessellations> refTessellations() const { return fTessellations; }

    template <typename FACTORY>
    sk_sp<SkVertices> find(const FACTORY& factory, const SkMatrix& matrix,
                           SkVector* translate) const {
        return fTessellations->find(factory, matrix, translate);
    }

private:
    std::unique_ptr<uint8_t[]> fKey;
    sk_sp<CachedTessellations> fTessellations;
};

template <typename FACTORY>
struct FindContext {
    SK_ONLY_STACK_ALLOCATABLE;

    FindContext(const SkMatrix* viewMatrix, const FACTORY* factory)
        : fViewMatrix(viewMatrix), fFactory(factory) {}
    const SkMatrix* const fViewMatrix;
    sk_sp<SkVertices> fVertices;
    SkVector fTranslate = {0, 0};
    sk_sp<CachedTessellations> fTessellationsOnFailure;
    const FACTORY* fFactory;
};

template <typename FACTORY>
bool FindVisitor(const SkResourceCache::Rec& baseRec, void* ctx) {
    FindContext<FACTORY>* findContext = (FindContext<FACTORY>*)ctx;
    const CachedTessellationsRec& rec = static_cast<const CachedTessellationsRec&>(baseRec);
    findContext->fVertices =
            rec.find(*findContext->fFactory, *findContext->fViewMatrix, &findContext->fTranslate);
    if (findContext->fVertices) {
        return true;
    }
    findContext->fTessellationsOnFailure = rec.refTessellations();
    return false;
}

class ShadowedPath {
public:
    SK_ONLY_STACK_ALLOCATABLE;

    ShadowedPath(const SkPath* path, const SkMatrix* viewMatrix)
        : fPath(path)
        , fViewMatrix(viewMatrix)
#if defined(SK_GANESH)
        , fShapeForKey(*path, GrStyle::SimpleFill())
#endif
    {}

    const SkPath& path() const { return *fPath; }
    const SkMatrix& viewMatrix() const { return *fViewMatrix; }
#if defined(SK_GANESH)
    int keyBytes() const {
        return fShapeForKey.hasUnstyledKey()
                       ? fShapeForKey.unstyledKeySize() * sizeof(uint32_t)
                       : -1;
    }
    void writeKey(void* key) const {
        fShapeForKey.writeUnstyledKey(reinterpret_cast<uint32_t*>(key));
    }
#else
    int keyBytes() const { return -1; }
    void writeKey(void*) const { SK_ABORT("Should never be called"); }
#endif

private:
    const SkPath* fPath;
    const SkMatrix* fViewMatrix;
#if defined(SK_GANESH)
    GrStyledShape fShapeForKey;
#endif
};

static void* kNamespace;

class ShadowInvalidator : public SkIDChangeListener {
public:
    explicit ShadowInvalidator(const SkResourceCache::Key& key) {
        fKey.reset(new uint8_t[key.size()]);
        memcpy(fKey.get(), &key, key.size());
    }

private:
    const SkResourceCache::Key& getKey() const {
        return *reinterpret_cast<SkResourceCache::Key*>(fKey.get());
    }

    static bool FindVisitor(const SkResourceCache::Rec&, void*) { return false; }

    void changed() override {
        SkResourceCache::Find(this->getKey(), ShadowInvalidator::FindVisitor, nullptr);
    }

    std::unique_ptr<uint8_t[]> fKey;
};

template <typename FACTORY>
bool draw_shadow(const FACTORY& factory,
                 std::function<void(const SkVertices*, SkBlendMode, const SkPaint&,
                                    SkScalar tx, SkScalar ty, bool)> drawProc,
                 ShadowedPath& path, SkColor color) {
    FindContext<FACTORY> context(&path.viewMatrix(), &factory);

    SkResourceCache::Key* key = nullptr;
    constexpr int kMinBytes = 128;
    AutoSTArray<kMinBytes / sizeof(SkResourceCache::Key), SkResourceCache::Key> keyStorage;
    int keyDataBytes = path.keyBytes();
    if (keyDataBytes >= 0) {
        keyStorage.reset(keyDataBytes + sizeof(SkResourceCache::Key));
        key = new (keyStorage.begin()) SkResourceCache::Key();
        path.writeKey((uint32_t*)(((uint8_t*)keyStorage.begin()) + sizeof(SkResourceCache::Key)));
        key->init(&kNamespace, resource_cache_shared_id(), keyDataBytes);
        SkResourceCache::Find(*key, FindVisitor<FACTORY>, &context);
    }

    sk_sp<SkVertices> vertices;
    bool foundInCache = SkToBool(context.fVertices);
    if (foundInCache) {
        vertices = std::move(context.fVertices);
    } else {
        if (key) {
            sk_sp<CachedTessellations> tessellations;
            if (context.fTessellationsOnFailure) {
                tessellations = std::move(context.fTessellationsOnFailure);
            } else {
                tessellations.reset(new CachedTessellations());
            }
            vertices = tessellations->add(path.path(), factory, path.viewMatrix(),
                                          &context.fTranslate);
            if (!vertices) {
                return false;
            }
            auto rec = new CachedTessellationsRec(*key, std::move(tessellations));
            SkPathPriv::AddGenIDChangeListener(path.path(), sk_make_sp<ShadowInvalidator>(*key));
            SkResourceCache::Add(rec);
        } else {
            vertices = factory.makeVertices(path.path(), path.viewMatrix(), &context.fTranslate);
            if (!vertices) {
                return false;
            }
        }
    }

    SkPaint paint;
    paint.setColorFilter(
            SkColorFilters::Blend(color, SkBlendMode::kModulate)->makeComposed(
                    SkColorFilterPriv::MakeGaussian()));

    drawProc(vertices.get(), SkBlendMode::kDst, paint,
             context.fTranslate.fX, context.fTranslate.fY, path.viewMatrix().hasPerspective());
    return true;
}

}  // namespace
#endif  // !defined(SK_ENABLE_OPTIMIZE_SIZE)

void SkShadowUtils::ComputeTonalColors(SkColor inAmbientColor, SkColor inSpotColor,
                                       SkColor* outAmbientColor, SkColor* outSpotColor) {
    skia_rs_shadow_compute_tonal_colors(inAmbientColor, inSpotColor, outAmbientColor, outSpotColor);
}

void SkShadowUtils::DrawShadow(SkCanvas* canvas, const SkPath& path, const SkPoint3& zPlaneParams,
                               const SkPoint3& lightPos, SkScalar lightRadius,
                               SkColor ambientColor, SkColor spotColor,
                               uint32_t flags) {
    SkDrawShadowRec rec;
    const SkMatrix ctm = canvas->getTotalMatrix();
    if (!skia_rs_shadow_fill_rec(zPlaneParams, lightPos, lightRadius, ambientColor, spotColor,
                                 flags, &ctm, &rec)) {
        return;
    }
    canvas->private_draw_shadow_rec(path, rec);
}

bool SkShadowUtils::GetLocalBounds(const SkMatrix& ctm, const SkPath& path,
                                   const SkPoint3& zPlaneParams, const SkPoint3& lightPos,
                                   SkScalar lightRadius, uint32_t flags, SkRect* bounds) {
    SkDrawShadowRec rec;
    if (!skia_rs_shadow_fill_rec(zPlaneParams, lightPos, lightRadius, SK_ColorBLACK, SK_ColorBLACK,
                                 flags, &ctm, &rec)) {
        return false;
    }
    SkDrawShadowMetrics::GetLocalBounds(path, rec, ctm, bounds);
    return true;
}

void SkDevice::drawShadow(SkCanvas* canvas, const SkPath& path, const SkDrawShadowRec& rec) {
    if (!skia_rs_shadow_validate_rec(&rec)) {
        return;
    }

    SkMatrix viewMatrix = this->localToDevice();

#if !defined(SK_ENABLE_OPTIMIZE_SIZE)
    auto drawVertsProc = [this](const SkVertices* vertices, SkBlendMode mode, const SkPaint& paint,
                                SkScalar tx, SkScalar ty, bool hasPerspective) {
        if (vertices->priv().vertexCount()) {
            SkAutoDeviceTransformRestore adr(
                    this,
                    hasPerspective ? SkM44()
                                   : this->localToDevice44() * SkM44::Translate(tx, ty));
            this->drawVertices(vertices, SkBlender::Mode(mode), paint, /*skipColorXform=*/true);
        }
    };

    ShadowedPath shadowedPath(&path, &viewMatrix);
    bool tiltZPlane = skia_rs_shadow_is_z_plane_tilted(rec.fZPlaneParams);
    bool transparent = SkToBool(rec.fFlags & SkShadowFlags::kTransparentOccluder_ShadowFlag);
    bool useBlur = SkToBool(rec.fFlags & SkShadowFlags::kConcaveBlurOnly_ShadowFlag) &&
                   !path.isConvex();
    bool uncached = tiltZPlane || path.isVolatile();
#endif
    bool directional = SkToBool(rec.fFlags & SkShadowFlags::kDirectionalLight_ShadowFlag);

    SkPoint3 zPlaneParams = rec.fZPlaneParams;
    SkPoint3 devLightPos = rec.fLightPos;
    if (!directional) {
        viewMatrix.mapPoints({(SkPoint*)&devLightPos.fX, 1});
    }
    float lightRadius = rec.fLightRadius;

    if (SkColorGetA(rec.fAmbientColor) > 0) {
        SkAutoDeviceTransformRestore adr(this, SkM44());
        bool success = false;
#if !defined(SK_ENABLE_OPTIMIZE_SIZE)
        if (uncached && !useBlur) {
            sk_sp<SkVertices> vertices = SkShadowTessellator::MakeAmbient(
                    path, viewMatrix, zPlaneParams, transparent);
            if (vertices) {
                SkPaint paint;
                paint.setColorFilter(
                        SkColorFilters::Blend(rec.fAmbientColor, SkBlendMode::kModulate)
                                ->makeComposed(SkColorFilterPriv::MakeGaussian()));
                this->drawVertices(vertices.get(), SkBlender::Mode(SkBlendMode::kDst), paint,
                                   /*skipColorXform=*/true);
                success = true;
            }
        }

        if (!success && !useBlur) {
            AmbientVerticesFactory factory;
            factory.fOccluderHeight = zPlaneParams.fZ;
            factory.fTransparent = transparent;
            if (viewMatrix.hasPerspective()) {
                factory.fOffset.set(0, 0);
            } else {
                factory.fOffset.fX = viewMatrix.getTranslateX();
                factory.fOffset.fY = viewMatrix.getTranslateY();
            }
            success = draw_shadow(factory, drawVertsProc, shadowedPath, rec.fAmbientColor);
        }
#endif

        if (!success) {
            SkPath devSpacePath = path.makeTransform(canvas->getLocalToDeviceAs3x3());
            devSpacePath.setIsVolatile(true);

            SkScalar blurRadius = 0.0f;
            SkScalar strokeWidth = 0.0f;
            skia_rs_shadow_ambient_blur_fallback_params(zPlaneParams.fZ, &blurRadius, &strokeWidth);

            SkAutoCanvasRestore autoRestore(canvas, /*doSave=*/true);
            canvas->setMatrix(SkM44());
            SkPaint paint;
            paint.setColor(rec.fAmbientColor);
            paint.setStrokeWidth(strokeWidth);
            paint.setStyle(SkPaint::kStrokeAndFill_Style);
            SkScalar sigma = SkBlurMask::ConvertRadiusToSigma(blurRadius);
            paint.setMaskFilter(SkMaskFilter::MakeBlur(kNormal_SkBlurStyle, sigma,
                                                       /*respectCTM=*/false));
            canvas->drawPath(devSpacePath, paint);
        }
    }

    if (SkColorGetA(rec.fSpotColor) > 0) {
        SkAutoDeviceTransformRestore adr(this, SkM44());
        bool success = false;
#if !defined(SK_ENABLE_OPTIMIZE_SIZE)
        if (uncached && !useBlur) {
            sk_sp<SkVertices> vertices = SkShadowTessellator::MakeSpot(
                    path, viewMatrix, zPlaneParams, devLightPos, lightRadius, transparent,
                    directional);
            if (vertices) {
                SkPaint paint;
                paint.setColorFilter(
                        SkColorFilters::Blend(rec.fSpotColor, SkBlendMode::kModulate)
                                ->makeComposed(SkColorFilterPriv::MakeGaussian()));
                this->drawVertices(vertices.get(), SkBlender::Mode(SkBlendMode::kDst), paint,
                                   /*skipColorXform=*/true);
                success = true;
            }
        }

        if (!success && !useBlur) {
            RsSpotFactoryParams sp = skia_rs_shadow_compute_spot_factory_params(
                    path.getBounds(), path.isConvex(), &viewMatrix, zPlaneParams.fZ,
                    devLightPos, lightRadius, transparent, directional);
            SpotVerticesFactory factory;
            factory.fOccluderHeight = zPlaneParams.fZ;
            factory.fDevLightPos = devLightPos;
            factory.fLightRadius = lightRadius;
            factory.fLocalCenter = sp.local_center;
            factory.fOffset = sp.offset;
            factory.fOccluderType = static_cast<SpotVerticesFactory::OccluderType>(sp.occluder_type);

            success = draw_shadow(factory, drawVertsProc, shadowedPath, rec.fSpotColor);
        }
#endif

        if (!success) {
            SkMatrix shadowMatrix;
            SkScalar radius;
            if (!SkDrawShadowMetrics::GetSpotShadowTransform(
                        devLightPos, lightRadius, canvas->getLocalToDeviceAs3x3(), zPlaneParams,
                        path.getBounds(), directional, &shadowMatrix, &radius)) {
                return;
            }
            SkAutoCanvasRestore autoRestore(canvas, /*doSave=*/true);
            canvas->setMatrix(shadowMatrix);
            SkPaint paint;
            paint.setColor(rec.fSpotColor);
            SkScalar sigma = SkBlurMask::ConvertRadiusToSigma(radius);
            paint.setMaskFilter(SkMaskFilter::MakeBlur(kNormal_SkBlurStyle, sigma,
                                                       /*respectCTM=*/false));
            canvas->drawPath(path, paint);
        }
    }
}
