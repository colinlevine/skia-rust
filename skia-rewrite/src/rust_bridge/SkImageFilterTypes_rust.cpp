/*
 * Safe Rust bridge for:
 *   src/core/SkImageFilterTypes.cpp
 * (Phase 1 Layer 4.4 skia-rs-imagefilters, GN target :core).
 */

#include "src/core/SkImageFilterTypes.h"

#include "include/core/SkAlphaType.h"
#include "include/core/SkBlendMode.h"
#include "include/core/SkBlender.h"
#include "include/core/SkCanvas.h"
#include "include/core/SkClipOp.h"
#include "include/core/SkColor.h"
#include "include/core/SkImage.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkM44.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPicture.h"
#include "include/core/SkShader.h"
#include "include/effects/SkRuntimeEffect.h"
#include "include/private/SkDebug.h"
#include "include/private/SkFloatingPoint.h"
#include "include/private/SkOnlyStackAllocatable.h"
#include "src/core/SkBitmapDevice.h"
#include "src/core/SkBlenderBase.h"
#include "src/core/SkBlurEngine.h"
#include "src/core/SkCanvasPriv.h"
#include "src/core/SkColorSpacePriv.h"
#include "src/core/SkDevice.h"
#include "src/core/SkImageFilterCache.h"
#include "src/core/SkImageFilter_Base.h"
#include "src/core/SkKnownRuntimeEffects.h"
#include "src/core/SkTraceEvent.h"
#include "src/effects/colorfilters/SkColorFilterBase.h"

#include <cstddef>
#include <cstdint>
#include <optional>
#include <utility>

static_assert(sizeof(SkMatrix) == 40);
static_assert(sizeof(SkM44) == 64);
static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkIRect) == 16);
static_assert(sizeof(SkPoint) == 8);
static_assert(sizeof(SkIPoint) == 8);
static_assert(sizeof(SkSize) == 8);
static_assert(sizeof(SkISize) == 8);
static_assert(sizeof(SkSamplingOptions) == 24);
static_assert(sizeof(skif::Mapping) == 192);
static_assert(sizeof(skif::Vector) == 8);
static_assert(sizeof(skif::IVector) == 8);

namespace {

struct RsFilterResultMeta {
    bool hasImage;
    int32_t boundary;
    SkSamplingOptions sampling;
    int32_t tileMode;
    SkMatrix transform;
    bool hasColorFilter;
    bool cfAffectsTransparentBlack;
    SkIRect layerBounds;
    SkISize imageDims;
    SkIRect imageSubset;
    SkISize backingDims;
};

struct RsCropPlan {
    int32_t action;  // 0=Empty, 1=PeriodicTransform, 2=Subset, 3=DecalAdjust, 4=ResolveAndTile
    int32_t effectiveTileMode;
    SkIRect fittedCrop;
    SkIPoint origin;
    SkMatrix periodicTransform;
    bool doubleClamp;
};

struct RsColorFilterPlan {
    int32_t action;  // 0=Empty, 1=SolidColor1x1, 2=ResolveAndClamp, 3=ComposeDeferred
    SkIRect bounds;
};

struct RsTransformPlan {
    int32_t action;  // 0=Empty, 1=MergeDeferred, 2=ResolveFirst
    SkSamplingOptions nextSampling;
    SkIRect resolveBounds;
    bool hasResolveBounds;
};

struct RsResolvePlan {
    int32_t action;  // 0=Empty, 1=Subset, 2=DrawToSurface
    SkIRect dstBounds;
    SkIPoint origin;
    int32_t boundary;
};

struct RsSubsetPlan {
    bool valid;
    SkIRect imageSubsetRel;
    SkIPoint origin;
};

struct RsAsShaderPlan {
    bool needsResolve;
    uint32_t analysis;
    SkSamplingOptions finalSampling;
};

struct RsShaderViewPlan {
    SkMatrix preDecal;
    SkMatrix postDecal;
    int32_t effectiveTileMode;
    bool usePixelOutset;
    int32_t outset;
    bool strict;
    bool requiresDecalInLayerSpace;
    SkRect decalBounds;
};

struct RsRescaleSetup {
    int32_t action;  // 0=Empty, 1=NoopAdjustBounds, 2=NoopSubset, 3=RunSteps
    SkIRect visibleLayerBounds;
    SkIPoint origin;
    SkIRect srcRect;
    int32_t tileMode;
    bool cfBorder;
    bool deferPeriodicTiling;
    bool preResolveDeferredXform;
    int32_t xSteps;
    int32_t ySteps;
    float finalScaleX;
    float finalScaleY;
};

struct RsRescaleStep {
    int32_t nextXSteps;
    int32_t nextYSteps;
    SkRect dstBoundsF;
    SkIRect dstPixelBounds;
    SkIRect sampleBounds;
    int32_t boundary;
    SkMatrix scaleXform;
    SkRect srcSampled;
};

struct RsTiledBorderPair {
    SkRect src;
    SkRect dst;
    SkMatrix matrix;
};

struct RsTiledBorderDraws {
    RsTiledBorderPair pairs[8];
};

struct RsMakeFromImagePlan {
    int32_t action;  // 0=Empty, 1=DirectSpecial, 2=DrawToSurface
    SkRect srcRect;
    SkRect dstRect;
    SkIRect srcRectInt;
    SkMatrix transform;
    SkIRect dstBounds;
};

struct RsBlurPlan {
    bool valid;
    SkIRect outputBounds;
    SkIRect sampleBounds;
    SkSize rescaleScale;
};

struct RsLowResBlurPlan {
    bool valid;
    SkSize lowResSigma;
    SkIRect srcRelativeOutput;
    SkIRect blurOutputBounds;
    int32_t tileMode;
    bool outsetSourceImage;
};

}  // namespace

extern "C" {
SkIRect skia_rs_if_round_out(SkRect r);
SkIRect skia_rs_if_round_in(SkRect r);
SkISize skia_rs_if_size_round(SkSize s);
SkISize skia_rs_if_size_ceil(SkSize s);
SkISize skia_rs_if_size_floor(SkSize s);
bool skia_rs_if_is_nearly_integer_translation(const SkMatrix* m, SkIPoint* out_origin);
bool skia_rs_if_mapping_decompose_ctm(
        skif::Mapping* mapping, const SkM44* ctm, int32_t capability, SkPoint representative_pt);
bool skia_rs_if_mapping_adjust_layer_space(skif::Mapping* mapping, const SkM44* layer);
SkRect skia_rs_if_mapping_map_rect(SkRect geom, const SkMatrix* matrix);
SkIRect skia_rs_if_mapping_map_irect(SkIRect geom, const SkMatrix* matrix);
SkIPoint skia_rs_if_mapping_map_ipoint(SkIPoint geom, const SkMatrix* matrix);
SkPoint skia_rs_if_mapping_map_point(SkPoint geom, const SkMatrix* matrix);
skif::Vector skia_rs_if_mapping_map_vector(skif::Vector geom, const SkMatrix* matrix);
skif::IVector skia_rs_if_mapping_map_ivector(skif::IVector geom, const SkMatrix* matrix);
SkSize skia_rs_if_mapping_map_size(SkSize geom, const SkMatrix* matrix);
SkISize skia_rs_if_mapping_map_isize(SkISize geom, const SkMatrix* matrix);
void skia_rs_if_mapping_map_matrix(const SkMatrix* m, const SkMatrix* matrix, SkMatrix* out);
bool skia_rs_if_inverse_map_rect(const SkMatrix* m, SkRect r, SkRect* out);
bool skia_rs_if_inverse_map_irect(const SkMatrix* m, SkIRect r, SkIRect* out);
SkIRect skia_rs_if_relevant_subset(SkIRect src, SkIRect dst, int32_t tile_mode);

bool skia_rs_if_auto_surface_padded_bounds(
        SkIRect dst_bounds, int32_t boundary, SkIRect* out_padded);
uint32_t skia_rs_if_analyze_bounds(
        const RsFilterResultMeta* meta, const SkMatrix* xtra, SkIRect dst_bounds, int32_t scope);
void skia_rs_if_plan_apply_crop(
        const RsFilterResultMeta* meta,
        SkIRect desired_output,
        SkIRect crop,
        int32_t tile_mode,
        RsCropPlan* out_plan);
RsColorFilterPlan skia_rs_if_plan_apply_color_filter(
        const RsFilterResultMeta* meta,
        SkIRect desired_output,
        bool new_cf_affects_transparent_black);
void skia_rs_if_plan_apply_transform(
        const RsFilterResultMeta* meta,
        SkIRect desired_output,
        const SkMatrix* transform,
        SkSamplingOptions sampling,
        RsTransformPlan* out_plan);
RsResolvePlan skia_rs_if_plan_resolve(
        const RsFilterResultMeta* meta, SkIRect dst_bounds, bool preserve_dst_bounds);
RsSubsetPlan skia_rs_if_plan_subset(
        SkISize image_dims,
        SkIPoint known_origin,
        SkIRect subset_bounds,
        bool clamp_src_if_disjoint);
int32_t skia_rs_if_subset_pixel_boundary(
        SkIRect orig_subset, SkIRect result_subset, int32_t orig_boundary);
void skia_rs_if_plan_as_shader(
        const RsFilterResultMeta* meta,
        SkSamplingOptions xtra_sampling,
        uint32_t flags,
        SkIRect sample_bounds,
        bool cf_is_simple_coeff_mode,
        bool color_spaces_equal,
        RsAsShaderPlan* out_plan);
void skia_rs_if_plan_shader_view(
        const RsFilterResultMeta* meta, uint32_t analysis, RsShaderViewPlan* out_plan);
void skia_rs_if_compute_rescale_setup(
        const RsFilterResultMeta* meta,
        SkIRect desired_output,
        SkSize scale,
        bool enforce_decal,
        bool allow_overscaling,
        bool color_type_matches,
        bool color_space_matches,
        RsRescaleSetup* out_setup);
void skia_rs_if_compute_rescale_step(
        SkIRect src_rect,
        SkRect step_bounds_f,
        int32_t x_steps,
        int32_t y_steps,
        float final_scale_x,
        float final_scale_y,
        int32_t tile_mode,
        bool defer_periodic_tiling,
        RsRescaleStep* out_step);
void skia_rs_if_compute_cf_border_rects(SkIRect border, SkIRect out_rects[4]);
void skia_rs_if_compute_tiled_border_draws(
        int32_t tile_mode,
        const SkMatrix* src_to_dst,
        SkRect src_border,
        SkRect dst_border,
        RsTiledBorderDraws* out_draws);
void skia_rs_if_plan_make_from_image(
        const skif::Mapping* mapping,
        SkIRect desired_output,
        SkISize image_dims,
        SkRect src_rect,
        SkRect dst_rect,
        RsMakeFromImagePlan* out_plan);
void skia_rs_if_plan_builder_blur(
        SkIRect desired_output,
        SkIRect input_layer_bounds,
        SkSize sigma,
        float max_sigma,
        RsBlurPlan* out_plan);
void skia_rs_if_plan_low_res_blur(
        SkSize sigma,
        float max_sigma,
        bool supports_only_decal,
        SkIRect output_bounds,
        SkISize low_res_dims,
        const SkMatrix* low_res_transform,
        int32_t low_res_tile_mode,
        int32_t low_res_boundary,
        RsLowResBlurPlan* out_plan);
}  // extern "C"

namespace skif {

namespace {

class RasterBackend : public Backend {
public:
    RasterBackend(const SkSurfaceProps& surfaceProps, SkColorType colorType)
            : Backend(SkImageFilterCache::Get(), surfaceProps, colorType) {}

    sk_sp<SkDevice> makeDevice(SkISize size,
                               sk_sp<SkColorSpace> colorSpace,
                               const SkSurfaceProps* props) const override {
        SkImageInfo imageInfo = SkImageInfo::Make(size,
                                                  this->colorType(),
                                                  kPremul_SkAlphaType,
                                                  std::move(colorSpace));
        return SkBitmapDevice::Create(imageInfo, props ? *props : this->surfaceProps());
    }

    sk_sp<SkSpecialImage> makeImage(const SkIRect& subset, sk_sp<SkImage> image) const override {
        return SkSpecialImages::MakeFromRaster(subset, image, this->surfaceProps());
    }

    sk_sp<SkImage> getCachedBitmap(const SkBitmap& data) const override {
        return SkImages::RasterFromBitmap(data);
    }

    const SkBlurEngine* getBlurEngine() const override {
        return SkBlurEngine::GetRasterBlurEngine();
    }
};

}  // namespace

///////////////////////////////////////////////////////////////////////////////////////////////////
// Backend & Stats
///////////////////////////////////////////////////////////////////////////////////////////////////

Backend::Backend(sk_sp<SkImageFilterCache> cache,
                 const SkSurfaceProps& surfaceProps,
                 const SkColorType colorType)
        : fCache(std::move(cache))
        , fSurfaceProps(surfaceProps)
        , fColorType(colorType) {}

Backend::~Backend() = default;

sk_sp<Backend> MakeRasterBackend(const SkSurfaceProps& surfaceProps, SkColorType colorType) {
    return sk_make_sp<RasterBackend>(surfaceProps, colorType);
}

void Stats::dumpStats() const {
    SkDebugf("ImageFilter Stats:\n"
             "      # visited filters: %d\n"
             "           # cache hits: %d\n"
             "   # offscreen surfaces: %d\n"
             " # shader-clamped draws: %d\n"
             "   # shader-tiled draws: %d\n",
             fNumVisitedImageFilters,
             fNumCacheHits,
             fNumOffscreenSurfaces,
             fNumShaderClampedDraws,
             fNumShaderBasedTilingDraws);
}

void Stats::reportStats() const {
    TRACE_EVENT_INSTANT2("skia", "ImageFilter Graph Size", TRACE_EVENT_SCOPE_THREAD,
                         "count", fNumVisitedImageFilters, "cache hits", fNumCacheHits);
    TRACE_EVENT_INSTANT1("skia", "ImageFilter Surfaces", TRACE_EVENT_SCOPE_THREAD,
                         "count", fNumOffscreenSurfaces);
    TRACE_EVENT_INSTANT2("skia", "ImageFilter Shader Tiling", TRACE_EVENT_SCOPE_THREAD,
                         "clamp", fNumShaderClampedDraws, "other", fNumShaderBasedTilingDraws);
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// Mapping & Rounding
///////////////////////////////////////////////////////////////////////////////////////////////////

SkIRect RoundOut(SkRect r) { return skia_rs_if_round_out(r); }

SkIRect RoundIn(SkRect r) { return skia_rs_if_round_in(r); }

bool Mapping::decomposeCTM(const SkM44& ctm, MatrixCapability capability,
                           const skif::ParameterSpace<SkPoint>& representativePt) {
    return skia_rs_if_mapping_decompose_ctm(
            this, &ctm, static_cast<int32_t>(capability), SkPoint(representativePt));
}

bool Mapping::decomposeCTM(const SkM44& ctm,
                           const SkImageFilter* filter,
                           const skif::ParameterSpace<SkPoint>& representativePt) {
    return this->decomposeCTM(
            ctm,
            filter ? as_IFB(filter)->getCTMCapability() : MatrixCapability::kComplex,
            representativePt);
}

bool Mapping::adjustLayerSpace(const SkM44& layer) {
    return skia_rs_if_mapping_adjust_layer_space(this, &layer);
}

template<>
SkRect Mapping::map<SkRect>(const SkRect& geom, const SkMatrix& matrix) {
    return skia_rs_if_mapping_map_rect(geom, &matrix);
}

template<>
SkIRect Mapping::map<SkIRect>(const SkIRect& geom, const SkMatrix& matrix) {
    return skia_rs_if_mapping_map_irect(geom, &matrix);
}

template<>
SkIPoint Mapping::map<SkIPoint>(const SkIPoint& geom, const SkMatrix& matrix) {
    return skia_rs_if_mapping_map_ipoint(geom, &matrix);
}

template<>
SkPoint Mapping::map<SkPoint>(const SkPoint& geom, const SkMatrix& matrix) {
    return skia_rs_if_mapping_map_point(geom, &matrix);
}

template<>
Vector Mapping::map<Vector>(const Vector& geom, const SkMatrix& matrix) {
    return skia_rs_if_mapping_map_vector(geom, &matrix);
}

template<>
IVector Mapping::map<IVector>(const IVector& geom, const SkMatrix& matrix) {
    return skia_rs_if_mapping_map_ivector(geom, &matrix);
}

template<>
SkSize Mapping::map<SkSize>(const SkSize& geom, const SkMatrix& matrix) {
    return skia_rs_if_mapping_map_size(geom, &matrix);
}

template<>
SkISize Mapping::map<SkISize>(const SkISize& geom, const SkMatrix& matrix) {
    return skia_rs_if_mapping_map_isize(geom, &matrix);
}

template<>
SkMatrix Mapping::map<SkMatrix>(const SkMatrix& m, const SkMatrix& matrix) {
    SkMatrix out;
    skia_rs_if_mapping_map_matrix(&m, &matrix, &out);
    return out;
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// LayerSpace<T>
///////////////////////////////////////////////////////////////////////////////////////////////////

LayerSpace<SkIRect> LayerSpace<SkIRect>::relevantSubset(const LayerSpace<SkIRect> dstRect,
                                                        SkTileMode tileMode) const {
    return LayerSpace<SkIRect>(skia_rs_if_relevant_subset(
            fData, SkIRect(dstRect), static_cast<int32_t>(tileMode)));
}

LayerSpace<SkISize> LayerSpace<SkSize>::round() const {
    return LayerSpace<SkISize>(skia_rs_if_size_round(fData));
}

LayerSpace<SkISize> LayerSpace<SkSize>::ceil() const {
    return LayerSpace<SkISize>(skia_rs_if_size_ceil(fData));
}

LayerSpace<SkISize> LayerSpace<SkSize>::floor() const {
    return LayerSpace<SkISize>(skia_rs_if_size_floor(fData));
}

LayerSpace<SkRect> LayerSpace<SkMatrix>::mapRect(const LayerSpace<SkRect>& r) const {
    return LayerSpace<SkRect>(Mapping::map(SkRect(r), fData));
}

LayerSpace<SkIRect> LayerSpace<SkMatrix>::mapRect(const LayerSpace<SkIRect>& r) const {
    return LayerSpace<SkIRect>(Mapping::map(SkIRect(r), fData));
}

LayerSpace<SkPoint> LayerSpace<SkMatrix>::mapPoint(const LayerSpace<SkPoint>& p) const {
    return LayerSpace<SkPoint>(Mapping::map(SkPoint(p), fData));
}

LayerSpace<Vector> LayerSpace<SkMatrix>::mapVector(const LayerSpace<Vector>& v) const {
    return LayerSpace<Vector>(Mapping::map(Vector(v), fData));
}

LayerSpace<SkSize> LayerSpace<SkMatrix>::mapSize(const LayerSpace<SkSize>& s) const {
    return LayerSpace<SkSize>(Mapping::map(SkSize(s), fData));
}

bool LayerSpace<SkMatrix>::inverseMapRect(const LayerSpace<SkRect>& r,
                                          LayerSpace<SkRect>* out) const {
    SkRect mapped;
    if (skia_rs_if_inverse_map_rect(&fData, SkRect(r), &mapped)) {
        *out = LayerSpace<SkRect>(mapped);
        return true;
    }
    return false;
}

bool LayerSpace<SkMatrix>::inverseMapRect(const LayerSpace<SkIRect>& rect,
                                          LayerSpace<SkIRect>* out) const {
    SkIRect mapped;
    if (skia_rs_if_inverse_map_irect(&fData, SkIRect(rect), &mapped)) {
        *out = LayerSpace<SkIRect>(mapped);
        return true;
    }
    return false;
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// FilterResult::AutoSurface
///////////////////////////////////////////////////////////////////////////////////////////////////

class FilterResult::AutoSurface {
public:
    SK_ONLY_STACK_ALLOCATABLE;

    AutoSurface(const Context& ctx,
                const LayerSpace<SkIRect>& dstBounds,
                PixelBoundary boundary,
                bool renderInParameterSpace,
                const SkSurfaceProps* props = nullptr)
            : fDstBounds(dstBounds)
            , fBoundary(boundary) {
        sk_sp<SkDevice> device = nullptr;
        SkIRect padded;
        if (skia_rs_if_auto_surface_padded_bounds(
                    SkIRect(dstBounds), static_cast<int32_t>(boundary), &padded)) {
            fDstBounds = LayerSpace<SkIRect>(padded);
            device = ctx.backend()->makeDevice(SkISize(fDstBounds.size()),
                                               ctx.refColorSpace(),
                                               props);
        }

        if (!device) {
            return;
        }

        ctx.markNewSurface();
        fCanvas.emplace(std::move(device));
        fCanvas->translate(-fDstBounds.left(), -fDstBounds.top());
        fCanvas->clear(SkColors::kTransparent);
        if (fBoundary == PixelBoundary::kTransparent) {
            fCanvas->clipIRect(SkIRect(dstBounds));
        } else {
            fCanvas->clipIRect(SkIRect(fDstBounds));
        }

        if (renderInParameterSpace) {
            fCanvas->concat(ctx.mapping().layerMatrix());
        }
    }

    explicit operator bool() const { return fCanvas.has_value(); }

    SkCanvas* canvas() { SkASSERT(fCanvas.has_value()); return &*fCanvas; }
    SkDevice* device() { return SkCanvasPriv::TopDevice(this->canvas()); }
    SkCanvas* operator->() { return this->canvas(); }

    FilterResult snap() {
        if (fCanvas.has_value()) {
            fCanvas->restoreToCount(0);
            this->device()->setImmutable();

            SkIRect subset = SkIRect::MakeWH(fDstBounds.width(), fDstBounds.height());
            sk_sp<SkSpecialImage> image = this->device()->snapSpecial(subset);
            fCanvas.reset();

            if (image && fBoundary != PixelBoundary::kUnknown) {
                const int padding = this->padding();
                subset = SkIRect::MakeSize(image->dimensions()).makeInset(padding, padding);
                LayerSpace<SkIPoint> origin{{fDstBounds.left() + padding,
                                             fDstBounds.top() + padding}};
                return {image->makeSubset(subset), origin, fBoundary};
            } else {
                return {image, fDstBounds.topLeft(), PixelBoundary::kUnknown};
            }
        } else {
            return {};
        }
    }

private:
    int padding() const { return fBoundary == PixelBoundary::kUnknown ? 0 : kLayerPadding; }

    std::optional<SkCanvas> fCanvas;
    LayerSpace<SkIRect> fDstBounds;
    PixelBoundary fBoundary;
};

///////////////////////////////////////////////////////////////////////////////////////////////////
// FilterResult helpers
///////////////////////////////////////////////////////////////////////////////////////////////////

namespace {

RsFilterResultMeta pack_meta(const SkSpecialImage* image,
                             int32_t boundary,
                             const SkSamplingOptions& sampling,
                             SkTileMode tileMode,
                             const SkMatrix& transform,
                             const SkColorFilter* colorFilter,
                             const SkIRect& layerBounds) {
    RsFilterResultMeta m{};
    m.hasImage = image != nullptr;
    m.boundary = boundary;
    m.sampling = sampling;
    m.tileMode = static_cast<int32_t>(tileMode);
    m.transform = transform;
    m.hasColorFilter = colorFilter != nullptr;
    m.cfAffectsTransparentBlack =
            colorFilter != nullptr && as_CFB(colorFilter)->affectsTransparentBlack();
    m.layerBounds = layerBounds;
    if (image) {
        m.imageDims = image->dimensions();
        m.imageSubset = image->subset();
        m.backingDims = image->backingStoreDimensions();
    } else {
        m.imageDims = {0, 0};
        m.imageSubset = SkIRect::MakeEmpty();
        m.backingDims = {0, 0};
    }
    return m;
}

void draw_color_filtered_border(SkCanvas* canvas,
                                LayerSpace<SkIRect> border,
                                sk_sp<SkColorFilter> colorFilter) {
    SkPaint cfOnly;
    cfOnly.setColor4f(SkColors::kTransparent);
    cfOnly.setColorFilter(std::move(colorFilter));
#if !defined(SK_USE_SRCOVER_FOR_FILTERS)
    cfOnly.setBlendMode(SkBlendMode::kSrc);
#endif
    SkIRect rects[4];
    skia_rs_if_compute_cf_border_rects(SkIRect(border), rects);
    for (int i = 0; i < 4; ++i) {
        canvas->drawIRect(rects[i], cfOnly);
    }
}

void draw_tiled_border(SkCanvas* canvas,
                       SkTileMode tileMode,
                       const SkPaint& paint,
                       const LayerSpace<SkMatrix>& srcToDst,
                       LayerSpace<SkRect> srcBorder,
                       LayerSpace<SkRect> dstBorder) {
    SkASSERT(tileMode != SkTileMode::kDecal);
    RsTiledBorderDraws draws;
    SkMatrix s2d = SkMatrix(srcToDst);
    skia_rs_if_compute_tiled_border_draws(
            static_cast<int32_t>(tileMode), &s2d, SkRect(srcBorder), SkRect(dstBorder), &draws);
    for (int i = 0; i < 8; ++i) {
        canvas->save();
        canvas->concat(draws.pairs[i].matrix);
        canvas->drawRect(draws.pairs[i].src, paint);
        canvas->restore();
    }
}

}  // namespace

///////////////////////////////////////////////////////////////////////////////////////////////////
// FilterResult methods
///////////////////////////////////////////////////////////////////////////////////////////////////

sk_sp<SkSpecialImage> FilterResult::imageAndOffset(const Context& ctx, SkIPoint* offset) const {
    auto [image, origin] = this->imageAndOffset(ctx);
    *offset = SkIPoint(origin);
    return image;
}

std::pair<sk_sp<SkSpecialImage>, LayerSpace<SkIPoint>> FilterResult::imageAndOffset(
        const Context& ctx) const {
    FilterResult resolved = this->resolve(ctx, ctx.desiredOutput());
    return {resolved.fImage, resolved.layerBounds().topLeft()};
}

FilterResult FilterResult::insetForSaveLayer() const {
    if (!fImage) {
        return {};
    }
    SkASSERT(fTileMode == SkTileMode::kDecal);
    FilterResult inset = this->insetByPixels(kLayerPadding);
    SkASSERT(inset.fBoundary == PixelBoundary::kInitialized &&
             inset.fTileMode == SkTileMode::kDecal);
    inset.fBoundary = PixelBoundary::kTransparent;
    return inset;
}

FilterResult FilterResult::insetByPixels(int n) const {
    auto insetBounds = fLayerBounds;
    insetBounds.inset(LayerSpace<SkISize>({n, n}));
    SkASSERT(!insetBounds.isEmpty());
    return this->subset(fLayerBounds.topLeft(), insetBounds);
}

SkEnumBitMask<FilterResult::BoundsAnalysis> FilterResult::analyzeBounds(
        const SkMatrix& xtraTransform,
        const SkIRect& dstBounds,
        BoundsScope scope) const {
    RsFilterResultMeta meta = pack_meta(fImage.get(), static_cast<int32_t>(fBoundary),
                                        fSamplingOptions, fTileMode, SkMatrix(fTransform),
                                        fColorFilter.get(), SkIRect(fLayerBounds));
    uint32_t mask = skia_rs_if_analyze_bounds(
            &meta, &xtraTransform, dstBounds, static_cast<int32_t>(scope));
    return SkEnumBitMask<BoundsAnalysis>(static_cast<BoundsAnalysis>(mask));
}

void FilterResult::updateTileMode(const Context& ctx, SkTileMode tileMode) {
    if (fImage) {
        fTileMode = tileMode;
        if (tileMode != SkTileMode::kDecal) {
            fLayerBounds = ctx.desiredOutput();
        }
    }
}

FilterResult FilterResult::applyCrop(const Context& ctx,
                                     const LayerSpace<SkIRect>& crop,
                                     SkTileMode tileMode) const {
    RsFilterResultMeta meta = pack_meta(fImage.get(), static_cast<int32_t>(fBoundary),
                                        fSamplingOptions, fTileMode, SkMatrix(fTransform),
                                        fColorFilter.get(), SkIRect(fLayerBounds));
    RsCropPlan plan;
    skia_rs_if_plan_apply_crop(&meta, SkIRect(ctx.desiredOutput()), SkIRect(crop),
                               static_cast<int32_t>(tileMode), &plan);
    SkTileMode effectiveTile = static_cast<SkTileMode>(plan.effectiveTileMode);
    switch (plan.action) {
        case 0:  // Empty
            return {};
        case 1:  // PeriodicTransform
            return this->applyTransform(ctx, LayerSpace<SkMatrix>(plan.periodicTransform),
                                        FilterResult::kDefaultSampling);
        case 2: {  // Subset
            FilterResult restrictedOutput = this->subset(
                    LayerSpace<SkIPoint>(plan.origin),
                    LayerSpace<SkIRect>(plan.fittedCrop),
                    plan.doubleClamp);
            restrictedOutput.updateTileMode(ctx, effectiveTile);
            if (restrictedOutput.fBoundary == PixelBoundary::kInitialized ||
                effectiveTile != SkTileMode::kDecal) {
                restrictedOutput.fBoundary = PixelBoundary::kUnknown;
            }
            return restrictedOutput;
        }
        case 3: {  // DecalAdjust
            FilterResult restrictedOutput = *this;
            restrictedOutput.fLayerBounds = LayerSpace<SkIRect>(plan.fittedCrop);
            return restrictedOutput;
        }
        case 4:
        default: {  // ResolveAndTile
            FilterResult tiled = this->resolve(ctx, LayerSpace<SkIRect>(plan.fittedCrop),
                                               /*preserveDstBounds=*/true);
            tiled.updateTileMode(ctx, effectiveTile);
            return tiled;
        }
    }
}

FilterResult FilterResult::applyColorFilter(const Context& ctx,
                                            sk_sp<SkColorFilter> colorFilter) const {
    SkASSERT(colorFilter);
    RsFilterResultMeta meta = pack_meta(fImage.get(), static_cast<int32_t>(fBoundary),
                                        fSamplingOptions, fTileMode, SkMatrix(fTransform),
                                        fColorFilter.get(), SkIRect(fLayerBounds));
    RsColorFilterPlan plan = skia_rs_if_plan_apply_color_filter(
            &meta, SkIRect(ctx.desiredOutput()), as_CFB(colorFilter)->affectsTransparentBlack());
    switch (plan.action) {
        case 0:  // Empty
            return {};
        case 1: {  // SolidColor1x1
            AutoSurface surface{ctx,
                                LayerSpace<SkIRect>(plan.bounds),
                                PixelBoundary::kInitialized,
                                /*renderInParameterSpace=*/false};
            if (surface) {
                SkPaint paint;
                paint.setColor4f(SkColors::kTransparent, /*colorSpace=*/nullptr);
                paint.setColorFilter(std::move(colorFilter));
#if !defined(SK_USE_SRCOVER_FOR_FILTERS)
                paint.setBlendMode(SkBlendMode::kSrc);
#endif
                surface->drawPaint(paint);
            }
            FilterResult solidColor = surface.snap();
            solidColor.updateTileMode(ctx, SkTileMode::kClamp);
            return solidColor;
        }
        case 2: {  // ResolveAndClamp
            FilterResult filtered = this->resolve(ctx, LayerSpace<SkIRect>(plan.bounds),
                                                  /*preserveDstBounds=*/true);
            filtered.fColorFilter = std::move(colorFilter);
            filtered.updateTileMode(ctx, SkTileMode::kClamp);
            return filtered;
        }
        case 3:
        default: {  // ComposeDeferred
            FilterResult filtered = *this;
            filtered.fLayerBounds = LayerSpace<SkIRect>(plan.bounds);
            filtered.fColorFilter = SkColorFilters::Compose(std::move(colorFilter), fColorFilter);
            return filtered;
        }
    }
}

FilterResult FilterResult::applyTransform(const Context& ctx,
                                          const LayerSpace<SkMatrix>& transform,
                                          const SkSamplingOptions& sampling) const {
    RsFilterResultMeta meta = pack_meta(fImage.get(), static_cast<int32_t>(fBoundary),
                                        fSamplingOptions, fTileMode, SkMatrix(fTransform),
                                        fColorFilter.get(), SkIRect(fLayerBounds));
    SkMatrix xform = SkMatrix(transform);
    RsTransformPlan plan;
    skia_rs_if_plan_apply_transform(&meta, SkIRect(ctx.desiredOutput()), &xform, sampling, &plan);
    if (plan.action == 0) {
        return {};
    }

    FilterResult transformed;
    if (plan.action == 1) {
        transformed = *this;
    } else {
        if (plan.hasResolveBounds) {
            transformed = this->resolve(ctx, LayerSpace<SkIRect>(plan.resolveBounds));
        }
        if (!transformed.fImage) {
            return {};
        }
    }

    transformed.fSamplingOptions = plan.nextSampling;
    transformed.fTransform.postConcat(transform);
    transformed.fLayerBounds = transform.mapRect(transformed.fLayerBounds);
    if (!LayerSpace<SkIRect>::Intersects(transformed.fLayerBounds, ctx.desiredOutput())) {
        return {};
    }

    return transformed;
}

FilterResult FilterResult::resolve(const Context& ctx,
                                   LayerSpace<SkIRect> dstBounds,
                                   bool preserveDstBounds) const {
    RsFilterResultMeta meta = pack_meta(fImage.get(), static_cast<int32_t>(fBoundary),
                                        fSamplingOptions, fTileMode, SkMatrix(fTransform),
                                        fColorFilter.get(), SkIRect(fLayerBounds));
    RsResolvePlan plan = skia_rs_if_plan_resolve(&meta, SkIRect(dstBounds), preserveDstBounds);
    if (plan.action == 0) {
        return {nullptr, {}};
    }
    if (plan.action == 1) {
        return this->subset(LayerSpace<SkIPoint>(plan.origin),
                            LayerSpace<SkIRect>(plan.dstBounds));
    }

    SkSurfaceProps props = {};
    AutoSurface surface{ctx,
                        LayerSpace<SkIRect>(plan.dstBounds),
                        static_cast<PixelBoundary>(plan.boundary),
                        /*renderInParameterSpace=*/false,
                        &props};
    if (surface) {
        this->draw(ctx, surface.device(), /*preserveDeviceState=*/false);
    }
    return surface.snap();
}

FilterResult FilterResult::subset(const LayerSpace<SkIPoint>& knownOrigin,
                                  const LayerSpace<SkIRect>& subsetBounds,
                                  bool clampSrcIfDisjoint) const {
    RsSubsetPlan plan = skia_rs_if_plan_subset(
            fImage->dimensions(), SkIPoint(knownOrigin), SkIRect(subsetBounds), clampSrcIfDisjoint);
    if (!plan.valid) {
        return {};
    }

    FilterResult result{fImage->makeSubset(plan.imageSubsetRel), LayerSpace<SkIPoint>(plan.origin)};
    result.fColorFilter = fColorFilter;
    result.fBoundary = static_cast<PixelBoundary>(skia_rs_if_subset_pixel_boundary(
            fImage->subset(), result.fImage->subset(), static_cast<int32_t>(fBoundary)));
    return result;
}

void FilterResult::draw(const Context& ctx, SkDevice* target, const SkBlender* blender) const {
    SkAutoDeviceTransformRestore adtr{target, ctx.mapping().layerToDevice()};
    this->draw(ctx, target, /*preserveDeviceState=*/true, blender);
}

void FilterResult::draw(const Context& ctx,
                        SkDevice* device,
                        bool preserveDeviceState,
                        const SkBlender* blender) const {
    const bool blendAffectsTransparentBlack = blender && as_BB(blender)->affectsTransparentBlack();
    if (!fImage) {
        if (blendAffectsTransparentBlack) {
            SkPaint clear;
            clear.setColor4f(SkColors::kTransparent);
            clear.setBlender(sk_ref_sp(blender));
            device->drawPaint(clear);
        }
        return;
    }

    BoundsScope scope = blendAffectsTransparentBlack ? BoundsScope::kShaderOnly
                                                     : BoundsScope::kCanDrawDirectly;
    SkEnumBitMask<BoundsAnalysis> analysis = this->analyzeBounds(device->localToDevice(),
                                                                 device->devClipBounds(),
                                                                 scope);

    if (analysis & BoundsAnalysis::kRequiresLayerCrop) {
        if (blendAffectsTransparentBlack) {
            LayerSpace<SkIRect> dstBounds;
            if (!LayerSpace<SkMatrix>(device->localToDevice()).inverseMapRect(
                        LayerSpace<SkIRect>(device->devClipBounds()), &dstBounds)) {
                return;
            }
            FilterResult clipped = this->resolve(ctx, dstBounds);
            clipped.draw(ctx, device, preserveDeviceState, blender);
            return;
        }
        if (preserveDeviceState) {
            device->pushClipStack();
        }
        device->clipRect(SkRect::Make(SkIRect(fLayerBounds)), SkClipOp::kIntersect, /*aa=*/true);
    }

    const SkMatrix transformMat = SkMatrix(fTransform);
    const SkMatrix devMat = device->localToDevice();
    const bool pixelAligned =
            skia_rs_if_is_nearly_integer_translation(&transformMat, nullptr) &&
            skia_rs_if_is_nearly_integer_translation(&devMat, nullptr);
    SkSamplingOptions sampling = fSamplingOptions;
    if (sampling == kDefaultSampling && pixelAligned) {
        sampling = {};
    }

    if (analysis & BoundsAnalysis::kHasLayerFillingEffect ||
        (blendAffectsTransparentBlack && (analysis & BoundsAnalysis::kDstBoundsNotCovered))) {
        SkPaint paint;
        if (!preserveDeviceState && !blender) {
#if !defined(SK_USE_SRCOVER_FOR_FILTERS)
            paint.setBlendMode(SkBlendMode::kSrc);
#endif
        } else {
            paint.setBlender(sk_ref_sp(blender));
        }
        paint.setShader(this->getAnalyzedShaderView(ctx, sampling, analysis));
        device->drawPaint(paint);
    } else {
        SkPaint paint;
        paint.setBlender(sk_ref_sp(blender));
        paint.setColorFilter(fColorFilter);

        SkMatrix netTransform = SkMatrix::Concat(device->localToDevice(), SkMatrix(fTransform));

        if (this->canClampToTransparentBoundary(analysis) && fSamplingOptions == kDefaultSampling) {
            SkASSERT(!(analysis & BoundsAnalysis::kRequiresShaderTiling));
            if (!preserveDeviceState && !blender) {
#if !defined(SK_USE_SRCOVER_FOR_FILTERS)
                paint.setBlendMode(SkBlendMode::kSrc);
#endif
            }
            netTransform.preTranslate(-1.f, -1.f);
            SkCanvas::SrcRectConstraint constraint = kLayerPadding > 1 || pixelAligned
                    ? SkCanvas::kFast_SrcRectConstraint
                    : SkCanvas::kStrict_SrcRectConstraint;
            device->drawSpecial(fImage->makePixelOutset().get(), netTransform, sampling, paint,
                                constraint);
        } else {
            paint.setAntiAlias(true);
            SkCanvas::SrcRectConstraint constraint = SkCanvas::kFast_SrcRectConstraint;
            if (analysis & BoundsAnalysis::kRequiresShaderTiling) {
                constraint = SkCanvas::kStrict_SrcRectConstraint;
                ctx.markShaderBasedTilingRequired(SkTileMode::kClamp);
            }
            device->drawSpecial(fImage.get(), netTransform, sampling, paint, constraint);
        }
    }

    if (preserveDeviceState && (analysis & BoundsAnalysis::kRequiresLayerCrop)) {
        device->popClipStack();
    }
}

sk_sp<SkShader> FilterResult::asShader(const Context& ctx,
                                       const SkSamplingOptions& xtraSampling,
                                       SkEnumBitMask<ShaderFlags> flags,
                                       const LayerSpace<SkIRect>& sampleBounds) const {
    if (!fImage) {
        return nullptr;
    }

    SkBlendMode colorFilterMode;
    const bool cfIsSimpleCoeff =
            !fColorFilter || (fColorFilter->asAColorMode(nullptr, &colorFilterMode) &&
                              colorFilterMode <= SkBlendMode::kLastCoeffMode);
    const bool colorSpacesEqual = SkColorSpace::Equals(fImage->getColorSpace(), ctx.colorSpace());

    RsFilterResultMeta meta = pack_meta(fImage.get(), static_cast<int32_t>(fBoundary),
                                        fSamplingOptions, fTileMode, SkMatrix(fTransform),
                                        fColorFilter.get(), SkIRect(fLayerBounds));
    RsAsShaderPlan plan;
    skia_rs_if_plan_as_shader(&meta, xtraSampling, static_cast<uint32_t>(flags.value()),
                              SkIRect(sampleBounds), cfIsSimpleCoeff, colorSpacesEqual, &plan);

    if (plan.needsResolve) {
        FilterResult resolved = this->resolve(ctx, sampleBounds);
        if (resolved) {
            auto analysis = resolved.analyzeBounds(sampleBounds, BoundsScope::kShaderOnly);
            return resolved.getAnalyzedShaderView(ctx, plan.finalSampling, analysis);
        }
        return nullptr;
    } else {
        auto analysis =
                SkEnumBitMask<BoundsAnalysis>(static_cast<BoundsAnalysis>(plan.analysis));
        return this->getAnalyzedShaderView(ctx, plan.finalSampling, analysis);
    }
}

sk_sp<SkShader> FilterResult::getAnalyzedShaderView(
        const Context& ctx,
        const SkSamplingOptions& finalSampling,
        SkEnumBitMask<BoundsAnalysis> analysis) const {
    RsFilterResultMeta meta = pack_meta(fImage.get(), static_cast<int32_t>(fBoundary),
                                        fSamplingOptions, fTileMode, SkMatrix(fTransform),
                                        fColorFilter.get(), SkIRect(fLayerBounds));
    RsShaderViewPlan plan;
    skia_rs_if_plan_shader_view(&meta, static_cast<uint32_t>(analysis.value()), &plan);

    SkTileMode effectiveTileMode = static_cast<SkTileMode>(plan.effectiveTileMode);
    sk_sp<SkShader> imageShader;
    if (plan.usePixelOutset) {
        imageShader = fImage->makePixelOutset(plan.outset)->asShader(
                SkTileMode::kClamp, finalSampling, plan.preDecal, plan.strict);
    } else {
        imageShader = fImage->asShader(effectiveTileMode, finalSampling, plan.preDecal, plan.strict);
    }
    if (plan.strict) {
        ctx.markShaderBasedTilingRequired(effectiveTileMode);
    }

    if (plan.requiresDecalInLayerSpace) {
        SkASSERT(fTileMode == SkTileMode::kDecal);
        const SkRuntimeEffect* decalEffect =
                GetKnownRuntimeEffect(SkKnownRuntimeEffects::StableKey::kDecal);
        SkRuntimeShaderBuilder builder(sk_ref_sp(decalEffect));
        builder.child("image") = std::move(imageShader);
        builder.uniform("decalBounds") = plan.decalBounds;
        imageShader = builder.makeShader();
    }

    if (imageShader && plan.requiresDecalInLayerSpace) {
        imageShader = imageShader->makeWithLocalMatrix(plan.postDecal);
    }

    if (imageShader && fColorFilter) {
        imageShader = imageShader->makeWithColorFilter(fColorFilter);
    }

    return imageShader;
}

FilterResult FilterResult::rescale(const Context& ctx,
                                   const LayerSpace<SkSize>& scale,
                                   bool enforceDecal,
                                   bool allowOverscaling) const {
    const SkColorSpace* srcCS = (fImage && fImage->getColorSpace()) ? fImage->getColorSpace()
                                                                    : sk_srgb_singleton();
    const SkColorSpace* dstCS = ctx.colorSpace() ? ctx.colorSpace() : srcCS;
    const bool colorTypeMatches = fImage && fImage->colorType() == ctx.backend()->colorType();
    const bool colorSpaceMatches = SkColorSpace::Equals(srcCS, dstCS);

    RsFilterResultMeta meta = pack_meta(fImage.get(), static_cast<int32_t>(fBoundary),
                                        fSamplingOptions, fTileMode, SkMatrix(fTransform),
                                        fColorFilter.get(), SkIRect(fLayerBounds));
    RsRescaleSetup setup;
    skia_rs_if_compute_rescale_setup(&meta, SkIRect(ctx.desiredOutput()), SkSize(scale),
                                     enforceDecal, allowOverscaling,
                                     colorTypeMatches, colorSpaceMatches, &setup);

    if (setup.action == 0) {
        return {};
    }
    if (setup.action == 1) {
        FilterResult noop = *this;
        noop.fLayerBounds = LayerSpace<SkIRect>(setup.visibleLayerBounds);
        return noop;
    }
    if (setup.action == 2) {
        return this->subset(LayerSpace<SkIPoint>(setup.origin),
                            LayerSpace<SkIRect>(setup.visibleLayerBounds));
    }

    LayerSpace<SkIRect> srcRect(setup.srcRect);
    SkTileMode tileMode = static_cast<SkTileMode>(setup.tileMode);
    bool cfBorder = setup.cfBorder;
    bool deferPeriodicTiling = setup.deferPeriodicTiling;
    LayerSpace<SkRect> stepBoundsF{srcRect};
    LayerSpace<SkRect> stepPixelBounds{srcRect};

    FilterResult image = *this;
    if (setup.preResolveDeferredXform) {
        image = image.resolve(ctx, srcRect);
        if (!image) {
            return {};
        }
        if (!cfBorder) {
            image.fTileMode = tileMode;
        }
    }
    if (deferPeriodicTiling) {
        image.fTileMode = SkTileMode::kClamp;
    }

    int32_t xSteps = setup.xSteps;
    int32_t ySteps = setup.ySteps;
    do {
        RsRescaleStep step;
        skia_rs_if_compute_rescale_step(
                SkIRect(srcRect), SkRect(stepBoundsF), xSteps, ySteps,
                setup.finalScaleX, setup.finalScaleY,
                static_cast<int32_t>(tileMode), deferPeriodicTiling, &step);
        xSteps = step.nextXSteps;
        ySteps = step.nextYSteps;

        LayerSpace<SkIRect> dstPixelBounds(step.dstPixelBounds);
        AutoSurface surface{ctx, dstPixelBounds, static_cast<PixelBoundary>(step.boundary),
                            /*renderInParameterSpace=*/false};
        if (surface) {
            auto analysis = image.analyzeBounds(step.scaleXform, step.sampleBounds,
                                                BoundsScope::kRescale);
            SkPaint paint;
            paint.setShader(image.getAnalyzedShaderView(ctx, image.sampling(), analysis));
#if !defined(SK_USE_SRCOVER_FOR_FILTERS)
            paint.setBlendMode(SkBlendMode::kSrc);
#endif
            surface->save();
                surface->concat(step.scaleXform);
                surface->drawRect(step.srcSampled, paint);
            surface->restore();

            if (cfBorder) {
                SkASSERT(fColorFilter && as_CFB(fColorFilter)->affectsTransparentBlack());
                SkASSERT(tileMode == SkTileMode::kClamp);
                draw_color_filtered_border(surface.canvas(), dstPixelBounds, fColorFilter);
                cfBorder = false;
            } else if (tileMode != SkTileMode::kDecal) {
                draw_tiled_border(surface.canvas(), tileMode, paint,
                                  LayerSpace<SkMatrix>(step.scaleXform),
                                  stepPixelBounds, LayerSpace<SkRect>(dstPixelBounds));
            }
        } else {
            return {};
        }

        image = surface.snap();
        image.fTileMode = deferPeriodicTiling ? SkTileMode::kClamp : tileMode;
        stepBoundsF = LayerSpace<SkRect>(step.dstBoundsF);
        stepPixelBounds = LayerSpace<SkRect>(dstPixelBounds);
    } while (xSteps > 0 || ySteps > 0);

    if (deferPeriodicTiling) {
        image = image.insetByPixels(kLayerPadding);
    }
    image.fTileMode = tileMode;
    image.fTransform.postConcat(
            LayerSpace<SkMatrix>::RectToRect(stepBoundsF, LayerSpace<SkRect>{srcRect}));
    image.fLayerBounds = LayerSpace<SkIRect>(setup.visibleLayerBounds);
    return image;
}

FilterResult FilterResult::MakeFromPicture(const Context& ctx,
                                           sk_sp<SkPicture> pic,
                                           ParameterSpace<SkRect> cullRect) {
    SkASSERT(pic);
    LayerSpace<SkIRect> dstBounds = ctx.mapping().paramToLayer(cullRect).roundOut();
    if (!dstBounds.intersect(ctx.desiredOutput())) {
        return {};
    }

    SkSurfaceProps props = ctx.backend()->surfaceProps()
                                         .cloneWithPixelGeometry(kUnknown_SkPixelGeometry);
    AutoSurface surface{ctx, dstBounds, PixelBoundary::kUnknown,
                        /*renderInParameterSpace=*/true, &props};
    if (surface) {
        surface->clipRect(SkRect(cullRect));
        surface->drawPicture(std::move(pic));
    }
    return surface.snap();
}

FilterResult FilterResult::MakeFromShader(const Context& ctx,
                                          sk_sp<SkShader> shader,
                                          bool dither) {
    SkASSERT(shader);
    PixelBoundary boundary = dither ? PixelBoundary::kUnknown : PixelBoundary::kTransparent;
    AutoSurface surface{ctx, ctx.desiredOutput(), boundary, /*renderInParameterSpace=*/true};
    if (surface) {
        SkPaint paint;
        paint.setShader(shader);
        paint.setDither(dither);
#if !defined(SK_USE_SRCOVER_FOR_FILTERS)
        paint.setBlendMode(SkBlendMode::kSrc);
#endif
        surface->drawPaint(paint);
    }
    return surface.snap();
}

FilterResult FilterResult::MakeFromImage(const Context& ctx,
                                         sk_sp<SkImage> image,
                                         SkRect srcRect,
                                         ParameterSpace<SkRect> dstRect,
                                         const SkSamplingOptions& sampling) {
    SkASSERT(image);
    RsMakeFromImagePlan plan;
    skia_rs_if_plan_make_from_image(&ctx.mapping(), SkIRect(ctx.desiredOutput()),
                                    image->dimensions(), srcRect, SkRect(dstRect), &plan);
    if (plan.action == 0) {
        return {};
    }
    if (plan.action == 1) {
        sk_sp<SkSpecialImage> specialImage =
                ctx.backend()->makeImage(plan.srcRectInt, std::move(image));
        FilterResult subset{std::move(specialImage),
                            LayerSpace<SkIPoint>(plan.srcRectInt.topLeft())};
        return subset.applyTransform(ctx, LayerSpace<SkMatrix>(plan.transform), sampling);
    }

    AutoSurface surface{ctx, LayerSpace<SkIRect>(plan.dstBounds), PixelBoundary::kTransparent,
                        /*renderInParameterSpace=*/true};
    if (surface) {
        SkPaint paint;
        paint.setAntiAlias(true);
        surface->drawImageRect(std::move(image), plan.srcRect, plan.dstRect, sampling, &paint,
                               SkCanvas::kStrict_SrcRectConstraint);
    }
    return surface.snap();
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// FilterResult::Builder
///////////////////////////////////////////////////////////////////////////////////////////////////

FilterResult::Builder::Builder(const Context& context) : fContext(context) {}
FilterResult::Builder::~Builder() = default;

SkSpan<sk_sp<SkShader>> FilterResult::Builder::createInputShaders(
        const LayerSpace<SkIRect>& outputBounds,
        bool evaluateInParameterSpace) {
    SkEnumBitMask<ShaderFlags> xtraFlags = ShaderFlags::kNone;
    SkMatrix layerToParam;
    if (evaluateInParameterSpace) {
        layerToParam = fContext.mapping().layerMatrix().asM33().invert().value_or(SkMatrix());
        if (!skia_rs_if_is_nearly_integer_translation(&layerToParam, nullptr)) {
            xtraFlags |= ShaderFlags::kNonTrivialSampling;
        }
    }

    fInputShaders.reserve(fInputs.size());
    for (const SampledFilterResult& input : fInputs) {
        auto sampleBounds = input.fSampleBounds ? *input.fSampleBounds : outputBounds;
        auto shader = input.fImage.asShader(fContext,
                                            input.fSampling,
                                            input.fFlags | xtraFlags,
                                            sampleBounds);
        if (evaluateInParameterSpace && shader) {
            shader = shader->makeWithLocalMatrix(layerToParam);
        }
        fInputShaders.push_back(std::move(shader));
    }
    return SkSpan<sk_sp<SkShader>>(fInputShaders);
}

LayerSpace<SkIRect> FilterResult::Builder::outputBounds(
        std::optional<LayerSpace<SkIRect>> explicitOutput) const {
    LayerSpace<SkIRect> output = fContext.desiredOutput();
    if (explicitOutput.has_value()) {
        if (!output.intersect(*explicitOutput)) {
            return LayerSpace<SkIRect>::Empty();
        }
    }
    return output;
}

FilterResult FilterResult::Builder::drawShader(sk_sp<SkShader> shader,
                                               const LayerSpace<SkIRect>& outputBounds,
                                               bool evaluateInParameterSpace) const {
    SkASSERT(!outputBounds.isEmpty());
    if (!shader) {
        return {};
    }

    AutoSurface surface{fContext, outputBounds, PixelBoundary::kTransparent,
                        evaluateInParameterSpace};
    if (surface) {
        SkPaint paint;
        paint.setShader(std::move(shader));
#if !defined(SK_USE_SRCOVER_FOR_FILTERS)
        paint.setBlendMode(SkBlendMode::kSrc);
#endif
        surface->drawPaint(paint);
    }
    return surface.snap();
}

FilterResult FilterResult::Builder::merge() {
    SkASSERT(!fInputs.empty());
    if (fInputs.size() == 1) {
        SkASSERT(!fInputs[0].fSampleBounds.has_value() &&
                 fInputs[0].fSampling == kDefaultSampling &&
                 fInputs[0].fFlags == ShaderFlags::kNone);
        return fInputs[0].fImage;
    }

    const auto mergedBounds = LayerSpace<SkIRect>::Union(
            (int) fInputs.size(),
            [this](int i) { return fInputs[i].fImage.layerBounds(); });
    const auto outputBounds = this->outputBounds(mergedBounds);

    AutoSurface surface{fContext, outputBounds, PixelBoundary::kTransparent,
                        /*renderInParameterSpace=*/false};
    if (surface) {
        for (const SampledFilterResult& input : fInputs) {
            SkASSERT(!input.fSampleBounds.has_value() &&
                     input.fSampling == kDefaultSampling &&
                     input.fFlags == ShaderFlags::kNone);
            input.fImage.draw(fContext, surface.device(), /*preserveDeviceState=*/true);
        }
    }
    return surface.snap();
}

FilterResult FilterResult::Builder::blur(const LayerSpace<SkSize>& sigma) {
    SkASSERT(fInputs.size() == 1);

    const SkBlurEngine* blurEngine = fContext.backend()->getBlurEngine();
    SkASSERT(blurEngine);

    const SkBlurEngine::Algorithm* algorithm = blurEngine->findAlgorithm(
            SkSize(sigma), fContext.backend()->colorType());
    if (!algorithm) {
        return {};
    }

    RsBlurPlan plan;
    skia_rs_if_plan_builder_blur(SkIRect(fContext.desiredOutput()),
                                 SkIRect(fInputs[0].fImage.layerBounds()),
                                 SkSize(sigma),
                                 algorithm->maxSigma(),
                                 &plan);
    if (!plan.valid) {
        return {};
    }

    LayerSpace<SkIRect> outputBounds(plan.outputBounds);
    FilterResult lowResImage = fInputs[0].fImage.rescale(
            fContext.withNewDesiredOutput(LayerSpace<SkIRect>(plan.sampleBounds)),
            LayerSpace<SkSize>(plan.rescaleScale),
            algorithm->supportsOnlyDecalTiling(),
            /*allowOverscaling=*/true);
    if (!lowResImage) {
        return {};
    }

    SkMatrix lowResXform = SkMatrix(lowResImage.fTransform);
    RsLowResBlurPlan lrPlan;
    skia_rs_if_plan_low_res_blur(SkSize(sigma),
                                 algorithm->maxSigma(),
                                 algorithm->supportsOnlyDecalTiling(),
                                 SkIRect(outputBounds),
                                 lowResImage.fImage->dimensions(),
                                 &lowResXform,
                                 static_cast<int32_t>(lowResImage.tileMode()),
                                 static_cast<int32_t>(lowResImage.fBoundary),
                                 &lrPlan);
    if (!lrPlan.valid) {
        return {};
    }

    sk_sp<SkSpecialImage> lowResBlur = lowResImage.refImage();
    if (lrPlan.outsetSourceImage) {
        lowResBlur = lowResBlur->makePixelOutset(kLayerPadding);
    }

    lowResBlur = algorithm->blur(lrPlan.lowResSigma,
                                 lowResBlur,
                                 SkIRect::MakeSize(lowResBlur->dimensions()),
                                 static_cast<SkTileMode>(lrPlan.tileMode),
                                 lrPlan.blurOutputBounds);
    if (!lowResBlur) {
        return {};
    }

    LayerSpace<SkIRect> srcRelativeOutput(lrPlan.srcRelativeOutput);
    FilterResult result{std::move(lowResBlur), srcRelativeOutput.topLeft()};
    if (lowResImage.tileMode() == SkTileMode::kClamp ||
        lowResImage.tileMode() == SkTileMode::kDecal) {
        result = result.insetByPixels(kLayerPadding);
    }

    result.fTransform.postConcat(lowResImage.fTransform);
    if (lowResImage.tileMode() == SkTileMode::kDecal) {
        outputBounds = this->outputBounds(
                result.fTransform.mapRect(LayerSpace<SkIRect>(result.fImage->dimensions())));
    }
    result.fLayerBounds = outputBounds;
    result.fTileMode = lowResImage.tileMode();
    return result;
}

}  // namespace skif
