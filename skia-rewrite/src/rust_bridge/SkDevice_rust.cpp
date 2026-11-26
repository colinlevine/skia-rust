/*
 * Safe Rust bridge for:
 *   src/core/SkDevice.cpp
 *   src/core/SkBitmapDevice.cpp
 *   src/core/SkClipStackDevice.cpp
 *   src/image/SkTiledImageUtils.cpp
 * (Phase 1 Layer 5.3 skia-rs-canvas, GN target :core).
 */

#include "include/core/SkAlphaType.h"
#include "include/core/SkBitmap.h"
#include "include/core/SkBlender.h"
#include "include/core/SkClipOp.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkColorType.h"
#include "include/core/SkDrawable.h"
#include "include/core/SkImage.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPath.h"
#include "include/core/SkPathBuilder.h"
#include "include/core/SkPathTypes.h"
#include "include/core/SkPixelRef.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRRect.h"
#include "include/core/SkRSXform.h"
#include "include/core/SkRasterHandleAllocator.h"
#include "include/core/SkRegion.h"
#include "include/core/SkScalar.h"
#include "include/core/SkShader.h"
#include "include/core/SkSpan.h"
#include "include/core/SkSurface.h"
#include "include/core/SkSurfaceProps.h"
#include "include/core/SkTileMode.h"
#include "include/core/SkTiledImageUtils.h"
#include "include/core/SkTypes.h"
#include "include/core/SkVertices.h"
#include "include/private/SkAssert.h"
#include "include/private/SkFloatingPoint.h"
#include "include/private/SkOnlyStackAllocatable.h"
#include "include/private/SkTFitsIn.h"
#include "include/private/SkTo.h"
#include "include/private/chromium/Slug.h"  // IWYU pragma: keep
#include "src/core/SkBitmapDevice.h"
#include "src/core/SkCanvasPriv.h"
#include "src/core/SkClipStackDevice.h"
#include "src/core/SkColorPriv.h"
#include "src/core/SkDevice.h"
#include "src/core/SkDraw.h"
#include "src/core/SkEnumerate.h"
#include "src/core/SkImageFilterTypes.h"
#include "src/core/SkImageFilter_Base.h"
#include "src/core/SkLatticeIter.h"
#include "src/core/SkMaskFilterBase.h"
#include "src/core/SkMatrixPriv.h"
#include "src/core/SkMemset.h"
#include "src/core/SkPathPriv.h"
#include "src/core/SkRasterClip.h"
#include "src/core/SkRectPriv.h"
#include "src/core/SkScalerContext.h"
#include "src/core/SkSpecialImage.h"
#include "src/cpu/Recorder.h"
#include "src/image/SkImage_Base.h"
#include "src/image/SkImage_Picture.h"
#include "src/image/SkImage_Raster.h"
#include "src/shaders/SkImageShader.h"
#include "src/text/GlyphRun.h"
#include "src/utils/SkPatchUtils.h"

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <utility>

namespace {

struct SkiaRsDeviceCoordSystem {
    SkM44 device_to_global;
    SkM44 global_to_device;
    SkM44 local_to_device;
    SkMatrix local_to_device_33;
};

struct SkiaRsDeviceGlobalCtmResult {
    SkM44 local_to_device;
    SkMatrix local_to_device_33;
};

struct SkiaRsNoPixelsClipState {
    SkIRect clip_bounds;
    bool is_aa;
    bool is_rect;
};

struct SkiaRsDrawTilerInit {
    bool needs_tiling;
    bool done;
    SkIRect src_bounds;
    SkIPoint origin;
};

struct SkiaRsDrawTilerStep {
    SkIPoint origin;
    bool done;
    SkIRect tile_bounds;
    SkMatrix tile_matrix;
};

struct SkiaRsBitmapDeviceValidation {
    bool valid;
    int32_t new_alpha_type;
};

struct SkiaRsBitmapImageRectPlan {
    int32_t action;
    SkMatrix matrix;
    SkRect tmp_src;
    SkRect dst_rect;
    bool use_tmp_dst;
    SkIRect src_ir;
};

static_assert(sizeof(SkM44) == 64);
static_assert(sizeof(SkMatrix) == 40);
static_assert(sizeof(SkIRect) == 16);
static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkIPoint) == 8);
static_assert(sizeof(SkPoint) == 8);
static_assert(sizeof(SkiaRsDeviceCoordSystem) == 232);
static_assert(sizeof(SkiaRsDeviceGlobalCtmResult) == 104);
static_assert(sizeof(SkiaRsNoPixelsClipState) == 20);
static_assert(sizeof(SkiaRsDrawTilerInit) == 28);
static_assert(sizeof(SkiaRsDrawTilerStep) == 68);
static_assert(sizeof(SkiaRsBitmapDeviceValidation) == 8);
static_assert(sizeof(SkiaRsBitmapImageRectPlan) == 96);

extern "C" {
void skia_rs_canvas_device_set_coord_system(
        const SkM44* device_to_global, const SkM44* global_to_device,
        const SkM44* local_to_device, int32_t buffer_origin_x, int32_t buffer_origin_y,
        SkiaRsDeviceCoordSystem* out);
void skia_rs_canvas_device_set_global_ctm(
        const SkM44* ctm, const SkM44* global_to_device, SkiaRsDeviceGlobalCtmResult* out);
bool skia_rs_canvas_device_is_pixel_aligned(const SkM44* device_to_global);
SkIPoint skia_rs_canvas_device_get_origin(const SkM44* device_to_global);
void skia_rs_canvas_device_get_relative_transform(
        const SkM44* src_device_to_global, const SkM44* dst_global_to_device, SkM44* out);
bool skia_rs_canvas_device_draw_region_needs_path(
        const SkMatrix* local_to_device, int32_t paint_style,
        bool has_mask_filter, bool has_path_effect, bool is_aa);
bool skia_rs_canvas_device_draw_device_is_strict(
        bool is_nearest, const SkMatrix* relative_transform);
uint32_t skia_rs_canvas_device_lattice_modulate_color(uint32_t color, uint8_t paint_alpha);
void skia_rs_canvas_device_quad_to_tris(const SkPoint* quad, SkPoint* tris_out);
uint32_t skia_rs_canvas_device_scaler_context_flags(bool has_cs, bool gamma_is_linear);
SkiaRsNoPixelsClipState skia_rs_canvas_no_pixels_clip_op(
        SkiaRsNoPixelsClipState state, int32_t op, const SkM44* transform,
        SkRect bounds, bool is_aa, bool fills_bounds);
SkIRect skia_rs_canvas_no_pixels_replace_clip(
        const SkM44* global_to_device, SkIRect rect, SkIRect device_bounds);
bool skia_rs_canvas_draw_tiler_needs_tiling(int32_t width, int32_t height);
SkiaRsDrawTilerInit skia_rs_canvas_draw_tiler_init(
        SkIRect clip_r, const SkMatrix* local_to_device, const SkRect* bounds);
void skia_rs_canvas_draw_tiler_step(
        SkIPoint origin, SkIRect src_bounds, const SkMatrix* local_to_device,
        SkiaRsDrawTilerStep* out);
SkiaRsBitmapDeviceValidation skia_rs_canvas_valid_for_bitmap_device(
        int32_t width, int32_t height, int32_t color_type, int32_t alpha_type,
        bool color_type_always_opaque);
bool skia_rs_canvas_can_apply_dst_matrix_as_ctm(const SkMatrix* m, bool has_mask_filter);
void skia_rs_canvas_bitmap_image_rect_plan(
        int32_t bm_width, int32_t bm_height, const SkRect* src, SkRect dst,
        bool is_fast_constraint, bool is_default_sampling, SkiaRsBitmapImageRectPlan* out);
bool skia_rs_canvas_bitmap_is_clip_wide_open(
        bool is_bw, bool bw_is_rect, SkIRect bw_bounds, int32_t width, int32_t height);
bool skia_rs_canvas_bitmap_is_clip_rect(bool rc_empty, bool rc_is_rect, bool has_clip_shader);
void skia_rs_canvas_tiled_image_bitmap_keys(
        uint32_t gen_id, SkIRect image_bounds, SkIPoint pixel_ref_origin, uint32_t* out_keys);
void skia_rs_canvas_tiled_image_unique_id_keys(uint32_t unique_id, uint32_t* out_keys);
}

}  // namespace

// ============================================================================
// SkDevice.cpp
// ============================================================================

SkDevice::SkDevice(const SkImageInfo& info, const SkSurfaceProps& surfaceProps)
        : fInfo(info)
        , fSurfaceProps(surfaceProps) {
    fDeviceToGlobal.setIdentity();
    fGlobalToDevice.setIdentity();
}

void SkDevice::setDeviceCoordinateSystem(const SkM44& deviceToGlobal,
                                         const SkM44& globalToDevice,
                                         const SkM44& localToDevice,
                                         int bufferOriginX,
                                         int bufferOriginY) {
    SkiaRsDeviceCoordSystem res;
    skia_rs_canvas_device_set_coord_system(
            &deviceToGlobal, &globalToDevice, &localToDevice, bufferOriginX, bufferOriginY, &res);
    fDeviceToGlobal = res.device_to_global;
    fGlobalToDevice = res.global_to_device;
    fLocalToDevice = res.local_to_device;
    fLocalToDevice33 = res.local_to_device_33;
    fLocalToDeviceDirty = true;
}

void SkDevice::setGlobalCTM(const SkM44& ctm) {
    SkiaRsDeviceGlobalCtmResult res;
    skia_rs_canvas_device_set_global_ctm(&ctm, &fGlobalToDevice, &res);
    fLocalToDevice = res.local_to_device;
    fLocalToDevice33 = res.local_to_device_33;
    fLocalToDeviceDirty = true;
}

bool SkDevice::isPixelAlignedToGlobal() const {
    return skia_rs_canvas_device_is_pixel_aligned(&fDeviceToGlobal);
}

SkIPoint SkDevice::getOrigin() const {
    SkASSERT(this->isPixelAlignedToGlobal());
    return skia_rs_canvas_device_get_origin(&fDeviceToGlobal);
}

SkM44 SkDevice::getRelativeTransform(const SkDevice& dstDevice) const {
    SkM44 out;
    skia_rs_canvas_device_get_relative_transform(&fDeviceToGlobal, &dstDevice.fGlobalToDevice, &out);
    return out;
}

void SkDevice::drawRegion(const SkRegion& region, const SkPaint& paint) {
    const SkMatrix& localToDevice = this->localToDevice();
    if (skia_rs_canvas_device_draw_region_needs_path(
                &localToDevice,
                static_cast<int32_t>(paint.getStyle()),
                paint.getMaskFilter() != nullptr,
                paint.getPathEffect() != nullptr,
                paint.isAntiAlias())) {
        SkPathBuilder builder;
        region.addBoundaryPath(&builder);
        builder.setIsVolatile(true);
        return this->drawPath(builder.detach(), paint);
    }

    SkRegion::Iterator it(region);
    while (!it.done()) {
        this->drawRect(SkRect::Make(it.rect()), paint);
        it.next();
    }
}

void SkDevice::drawArc(const SkArc& arc, const SkPaint& paint) {
    bool isFillNoPathEffect = SkPaint::kFill_Style == paint.getStyle() && !paint.getPathEffect();
    SkPath path = SkPathPriv::CreateDrawArcPath(arc, isFillNoPathEffect);
    this->drawPath(path, paint);
}

void SkDevice::drawDRRect(const SkRRect& outer, const SkRRect& inner, const SkPaint& paint) {
    SkPathBuilder builder;
    builder.addRRect(outer);
    builder.addRRect(inner);
    builder.setFillType(SkPathFillType::kEvenOdd);
    builder.setIsVolatile(true);
    this->drawPath(builder.detach(), paint);
}

void SkDevice::drawPatch(const SkPoint cubics[12], const SkColor colors[4],
                         const SkPoint texCoords[4], sk_sp<SkBlender> blender,
                         const SkPaint& paint) {
    SkISize lod = SkPatchUtils::GetLevelOfDetail(cubics, &this->localToDevice());
    auto vertices = SkPatchUtils::MakeVertices(cubics, colors, texCoords, lod.width(), lod.height(),
                                               this->imageInfo().colorSpace());
    if (vertices) {
        this->drawVertices(vertices.get(), std::move(blender), paint);
    }
}

void SkDevice::drawImageLattice(const SkImage* image, const SkCanvas::Lattice& lattice,
                                const SkRect& dst, SkFilterMode filter, const SkPaint& paint) {
    SkLatticeIter iter(lattice, dst);

    SkRect srcR, dstR;
    SkColor c;
    bool isFixedColor = false;
    const SkImageInfo info = SkImageInfo::Make(1, 1, kBGRA_8888_SkColorType, kUnpremul_SkAlphaType);

    while (iter.next(&srcR, &dstR, &isFixedColor, &c)) {
        if (isFixedColor || (srcR.width() <= 1.0f && srcR.height() <= 1.0f &&
                             image->readPixels(nullptr, info, &c, 4, srcR.fLeft, srcR.fTop))) {
            if (0 != c || !paint.isSrcOver()) {
                SkPaint paintCopy(paint);
                paintCopy.setColor(
                        skia_rs_canvas_device_lattice_modulate_color(c, paint.getAlpha()));
                this->drawRect(dstR, paintCopy);
            }
        } else {
            this->drawImageRect(image, &srcR, dstR, SkSamplingOptions(filter), paint,
                                SkCanvas::kStrict_SrcRectConstraint);
        }
    }
}

static SkPoint* quad_to_tris(SkPoint tris[6], SkSpan<const SkPoint> quad) {
    SkASSERT(quad.size() == 4);
    skia_rs_canvas_device_quad_to_tris(quad.data(), tris);
    return tris + 6;
}

void SkDevice::drawAtlas(SkSpan<const SkRSXform> xform,
                         SkSpan<const SkRect> tex,
                         SkSpan<const SkColor> colors,
                         sk_sp<SkBlender> blender,
                         const SkPaint& paint) {
    const size_t quadCount = xform.size();
    const size_t triCount = quadCount << 1;
    const size_t vertexCount = triCount * 3;
    uint32_t flags = SkVertices::kHasTexCoords_BuilderFlag;
    if (!colors.empty()) {
        flags |= SkVertices::kHasColors_BuilderFlag;
    }
    SkVertices::Builder builder(SkVertices::kTriangles_VertexMode, vertexCount, 0, flags);

    SkPoint* vPos = builder.positions();
    SkPoint* vTex = builder.texCoords();
    SkColor* vCol = builder.colors();
    for (size_t i = 0; i < quadCount; ++i) {
        SkPoint tmp[4];
        xform[i].toQuad(tex[i].width(), tex[i].height(), tmp);
        vPos = quad_to_tris(vPos, tmp);
        vTex = quad_to_tris(vTex, tex[i].toQuad());

        if (!colors.empty()) {
            SkOpts::memset32(vCol, colors[i], 6);
            vCol += 6;
        }
    }
    this->drawVertices(builder.detach().get(), std::move(blender), paint);
}

void SkDevice::drawEdgeAAQuad(const SkRect& r, const SkPoint clip[4], SkCanvas::QuadAAFlags aa,
                              const SkColor4f& color, SkBlendMode mode) {
    SkPaint paint;
    paint.setColor4f(color);
    paint.setBlendMode(mode);
    paint.setAntiAlias(aa == SkCanvas::kAll_QuadAAFlags);

    if (clip) {
        this->drawPath(SkPath::Polygon({clip, 4}, true), paint);
    } else {
        this->drawRect(r, paint);
    }
}

void SkDevice::drawEdgeAAImageSet(const SkCanvas::ImageSetEntry images[], int count,
                                  const SkPoint dstClips[], const SkMatrix preViewMatrices[],
                                  const SkSamplingOptions& sampling, const SkPaint& paint,
                                  SkCanvas::SrcRectConstraint constraint) {
    SkASSERT(paint.getStyle() == SkPaint::kFill_Style);
    SkASSERT(!paint.getPathEffect());

    SkPaint entryPaint = paint;
    const SkM44 baseLocalToDevice = this->localToDevice44();
    int clipIndex = 0;
    for (int i = 0; i < count; ++i) {
        entryPaint.setAntiAlias(images[i].fAAFlags == SkCanvas::kAll_QuadAAFlags);
        entryPaint.setAlphaf(paint.getAlphaf() * images[i].fAlpha);

        SkASSERT(images[i].fMatrixIndex < 0 || preViewMatrices);
        if (images[i].fMatrixIndex >= 0) {
            this->setLocalToDevice(baseLocalToDevice *
                                   SkM44(preViewMatrices[images[i].fMatrixIndex]));
        }

        SkASSERT(!images[i].fHasClip || dstClips);
        if (images[i].fHasClip) {
            this->pushClipStack();
            SkPath clipPath = SkPath::Polygon({dstClips + clipIndex, 4}, true);
            this->clipPath(clipPath, SkClipOp::kIntersect, entryPaint.isAntiAlias());
            clipIndex += 4;
        }
        this->drawImageRect(images[i].fImage.get(), &images[i].fSrcRect, images[i].fDstRect,
                            sampling, entryPaint, constraint);
        if (images[i].fHasClip) {
            this->popClipStack();
        }
        if (images[i].fMatrixIndex >= 0) {
            this->setLocalToDevice(baseLocalToDevice);
        }
    }
}

void SkDevice::drawDrawable(SkCanvas* canvas, SkDrawable* drawable, const SkMatrix* matrix) {
    drawable->draw(canvas, matrix);
}

void SkDevice::drawSpecial(SkSpecialImage*, const SkMatrix&, const SkSamplingOptions&,
                           const SkPaint&, SkCanvas::SrcRectConstraint) {}

void SkDevice::drawCoverageMask(const SkSpecialImage*, const SkMatrix&,
                                const SkSamplingOptions&, const SkPaint&) {
    SK_ABORT("Must override if useDrawCoverageMaskForMaskFilters() is true");
}

sk_sp<SkSpecialImage> SkDevice::snapSpecial(const SkIRect&, bool) { return nullptr; }
sk_sp<SkSpecialImage> SkDevice::snapSpecialScaled(const SkIRect&, const SkISize&) {
    return nullptr;
}
sk_sp<SkSpecialImage> SkDevice::snapSpecial() {
    return this->snapSpecial(SkIRect::MakeWH(this->width(), this->height()));
}

sk_sp<skif::Backend> SkDevice::createImageFilteringBackend(const SkSurfaceProps& surfaceProps,
                                                           SkColorType colorType) const {
    return skif::MakeRasterBackend(surfaceProps, colorType);
}

void SkDevice::drawDevice(SkDevice* device,
                          const SkSamplingOptions& sampling,
                          const SkPaint& paint) {
    sk_sp<SkSpecialImage> deviceImage = device->snapSpecial();
    if (deviceImage) {
        SkMatrix relativeTransform = device->getRelativeTransform(*this).asM33();
        const bool strict = skia_rs_canvas_device_draw_device_is_strict(
                sampling == SkFilterMode::kNearest, &relativeTransform);
        this->drawSpecial(deviceImage.get(), relativeTransform, sampling, paint,
                          strict ? SkCanvas::kStrict_SrcRectConstraint
                                 : SkCanvas::kFast_SrcRectConstraint);
    }
}

void SkDevice::drawFilteredImage(const skif::Mapping& mapping,
                                 SkSpecialImage* src,
                                 SkColorType colorType,
                                 const SkImageFilter* filter,
                                 const SkSamplingOptions& sampling,
                                 const SkPaint& paint) {
    SkASSERT(!paint.getImageFilter() && !paint.getMaskFilter());

    skif::LayerSpace<SkIRect> targetOutput = mapping.deviceToLayer(
            skif::DeviceSpace<SkIRect>(this->devClipBounds()));

    if (colorType == kUnknown_SkColorType) {
        colorType = kRGBA_8888_SkColorType;
    }

    skif::Stats stats;
    skif::Context ctx{this->createImageFilteringBackend(src ? src->props() : this->surfaceProps(),
                                                        colorType),
                      mapping,
                      targetOutput,
                      skif::FilterResult(sk_ref_sp(src)),
                      this->imageInfo().colorSpace(),
                      &stats};

    SkIPoint offset;
    sk_sp<SkSpecialImage> result = as_IFB(filter)->filterImage(ctx).imageAndOffset(ctx, &offset);
    stats.reportStats();
    if (result) {
        SkMatrix deviceMatrixWithOffset = mapping.layerToDevice().asM33();
        deviceMatrixWithOffset.preTranslate(offset.fX, offset.fY);
        this->drawSpecial(result.get(), deviceMatrixWithOffset, sampling, paint);
    }
}

bool SkDevice::accessPixels(SkPixmap* pmap) {
    SkPixmap tempStorage;
    if (nullptr == pmap) {
        pmap = &tempStorage;
    }
    return this->onAccessPixels(pmap);
}

bool SkDevice::peekPixels(SkPixmap* pmap) {
    SkPixmap tempStorage;
    if (nullptr == pmap) {
        pmap = &tempStorage;
    }
    return this->onPeekPixels(pmap);
}

static sk_sp<SkShader> make_post_inverse_lm(const SkShader* shader, const SkMatrix& lm) {
    auto inverse_lm = lm.invert();
    if (!shader || !inverse_lm) {
        return nullptr;
    }
#if defined(SK_BUILD_FOR_ANDROID_FRAMEWORK)
    SkMatrix prev_local_matrix;
    const auto nested_shader = as_SB(shader)->makeAsALocalMatrixShader(&prev_local_matrix);
    if (nested_shader) {
        shader = nested_shader.get();
    }
    return shader->makeWithLocalMatrix(*inverse_lm * prev_local_matrix);
#endif
    return shader->makeWithLocalMatrix(*inverse_lm);
}

void SkDevice::drawGlyphRunList(SkCanvas* canvas,
                                const sktext::GlyphRunList& glyphRunList,
                                const SkPaint& paint) {
    if (!this->localToDevice().isFinite()) {
        return;
    }

    if (!glyphRunList.hasRSXForm()) {
        this->onDrawGlyphRunList(canvas, glyphRunList, paint);
    } else {
        this->simplifyGlyphRunRSXFormAndRedraw(canvas, glyphRunList, paint);
    }
}

void SkDevice::simplifyGlyphRunRSXFormAndRedraw(SkCanvas* canvas,
                                                const sktext::GlyphRunList& glyphRunList,
                                                const SkPaint& paint) {
    for (const sktext::GlyphRun& run : glyphRunList) {
        if (run.scaledRotations().empty()) {
            auto subList = glyphRunList.builder()->makeGlyphRunList(run, paint, {0, 0});
            this->drawGlyphRunList(canvas, subList, paint);
        } else {
            SkPoint origin = glyphRunList.origin();
            SkPoint sharedPos{0, 0};
            SkGlyphID sharedGlyphID;
            sktext::GlyphRun glyphRun{
                    run.font(),
                    SkSpan<const SkPoint>{&sharedPos, 1},
                    SkSpan<const SkGlyphID>{&sharedGlyphID, 1},
                    SkSpan<const char>{},
                    SkSpan<const uint32_t>{},
                    SkSpan<const SkVector>{}};

            for (auto [i, glyphID, pos] : SkMakeEnumerate(run.source())) {
                sharedGlyphID = glyphID;
                auto [scos, ssin] = run.scaledRotations()[i];
                SkRSXform rsxForm = SkRSXform::Make(scos, ssin, pos.x(), pos.y());
                SkMatrix glyphToLocal;
                glyphToLocal.setRSXform(rsxForm).postTranslate(origin.x(), origin.y());

                SkPaint invertingPaint{paint};
                invertingPaint.setShader(make_post_inverse_lm(paint.getShader(), glyphToLocal));
                SkAutoCanvasRestore acr(canvas, true);
                canvas->concat(SkM44(glyphToLocal));
                sktext::GlyphRunList subList =
                        glyphRunList.builder()->makeGlyphRunList(glyphRun, paint, {0, 0});
                this->drawGlyphRunList(canvas, subList, invertingPaint);
            }
        }
    }
}

sk_sp<sktext::gpu::Slug> SkDevice::convertGlyphRunListToSlug(
        const sktext::GlyphRunList&, const SkPaint&) {
    return nullptr;
}

void SkDevice::drawSlug(SkCanvas*, const sktext::gpu::Slug*, const SkPaint&) {
    SK_ABORT("Slug drawing not supported.");
}

sk_sp<SkSurface> SkDevice::makeSurface(SkImageInfo const&, SkSurfaceProps const&) {
    return nullptr;
}

SkScalerContextFlags SkDevice::scalerContextFlags() const {
    const SkColorSpace* const cs = fInfo.colorSpace();
    return static_cast<SkScalerContextFlags>(
            skia_rs_canvas_device_scaler_context_flags(cs != nullptr, cs && cs->gammaIsLinear()));
}

SkNoPixelsDevice::SkNoPixelsDevice(const SkIRect& bounds, const SkSurfaceProps& props)
        : SkNoPixelsDevice(bounds, props, nullptr) {}

SkNoPixelsDevice::SkNoPixelsDevice(const SkIRect& bounds, const SkSurfaceProps& props,
                                   sk_sp<SkColorSpace> colorSpace)
        : SkDevice(SkImageInfo::Make(bounds.size(), kUnknown_SkColorType, kUnknown_SkAlphaType,
                                     std::move(colorSpace)),
                   props) {
    this->setOrigin(SkM44(), bounds.left(), bounds.top());
    fClipStack.emplace_back(this->bounds(), /*isAA=*/false, /*isRect=*/true);
}

bool SkNoPixelsDevice::resetForNextPicture(const SkIRect& bounds) {
    SkASSERT(this->isPixelAlignedToGlobal());
    if (bounds.width() != this->width() || bounds.height() != this->height()) {
        return false;
    }

    SkASSERT(fClipStack.size() == 1 && fClipStack[0].fDeferredSaveCount == 0);
    fClipStack[0].fClipBounds = this->bounds();
    fClipStack[0].fIsAA = false;
    fClipStack[0].fIsRect = true;

    this->setOrigin(SkM44(), bounds.left(), bounds.top());
    return true;
}

void SkNoPixelsDevice::pushClipStack() {
    SkASSERT(!fClipStack.empty());
    fClipStack.back().fDeferredSaveCount++;
}

void SkNoPixelsDevice::popClipStack() {
    SkASSERT(!fClipStack.empty());
    if (fClipStack.back().fDeferredSaveCount > 0) {
        fClipStack.back().fDeferredSaveCount--;
    } else {
        fClipStack.pop_back();
        SkASSERT(!fClipStack.empty());
    }
}

SkNoPixelsDevice::ClipState& SkNoPixelsDevice::writableClip() {
    SkASSERT(!fClipStack.empty());
    ClipState& current = fClipStack.back();
    if (current.fDeferredSaveCount > 0) {
        current.fDeferredSaveCount--;
        SkIRect bounds = current.fClipBounds;
        bool aa = current.fIsAA;
        bool rect = current.fIsRect;
        return fClipStack.emplace_back(bounds, aa, rect);
    } else {
        return current;
    }
}

void SkNoPixelsDevice::clipRect(const SkRect& rect, SkClipOp op, bool aa) {
    this->writableClip().op(op, this->localToDevice44(), rect, aa, /*fillsBounds=*/true);
}

void SkNoPixelsDevice::clipRRect(const SkRRect& rrect, SkClipOp op, bool aa) {
    this->writableClip().op(op, this->localToDevice44(), rrect.getBounds(),
                            aa, /*fillsBounds=*/rrect.isRect());
}

void SkNoPixelsDevice::clipPath(const SkPath& path, SkClipOp op, bool aa) {
    if (path.isInverseFillType()) {
        op = (op == SkClipOp::kDifference ? SkClipOp::kIntersect : SkClipOp::kDifference);
    }
    this->writableClip().op(op, this->localToDevice44(), path.getBounds(),
                            aa, /*fillsBounds=*/false);
}

void SkNoPixelsDevice::clipRegion(const SkRegion& globalRgn, SkClipOp op) {
    this->writableClip().op(op, this->globalToDevice(), SkRect::Make(globalRgn.getBounds()),
                            /*isAA=*/false, /*fillsBounds=*/globalRgn.isRect());
}

void SkNoPixelsDevice::onClipShader(sk_sp<SkShader>) {
    this->writableClip().fIsRect = false;
}

void SkNoPixelsDevice::replaceClip(const SkIRect& rect) {
    SkIRect deviceRect = skia_rs_canvas_no_pixels_replace_clip(
            &this->globalToDevice(), rect, this->bounds());
    auto& clip = this->writableClip();
    clip.fClipBounds = deviceRect;
    clip.fIsRect = true;
    clip.fIsAA = false;
}

void SkNoPixelsDevice::ClipState::op(SkClipOp op, const SkM44& transform, const SkRect& bounds,
                                     bool isAA, bool fillsBounds) {
    SkiaRsNoPixelsClipState in{fClipBounds, fIsAA, fIsRect};
    SkiaRsNoPixelsClipState out = skia_rs_canvas_no_pixels_clip_op(
            in, static_cast<int32_t>(op), &transform, bounds, isAA, fillsBounds);
    fClipBounds = out.clip_bounds;
    fIsAA = out.is_aa;
    fIsRect = out.is_rect;
}

// ============================================================================
// SkBitmapDevice.cpp
// ============================================================================

struct Bounder {
    SkRect fBounds;
    bool fHasBounds;

    Bounder(const SkRect& r, const SkPaint& paint) {
        if ((fHasBounds = paint.canComputeFastBounds())) {
            fBounds = paint.computeFastBounds(r, &fBounds);
        }
    }

    bool hasBounds() const { return fHasBounds; }
    const SkRect* bounds() const { return fHasBounds ? &fBounds : nullptr; }
    operator const SkRect*() const { return this->bounds(); }
};

class SkDrawTiler {
    SK_ONLY_STACK_ALLOCATABLE;

    SkBitmapDevice* fDevice;
    SkPixmap fRootPixmap;
    SkIRect fSrcBounds;

    skcpu::Draw fDraw;

    std::optional<SkMatrix> fTileMatrix;
    SkRasterClip fTileRC;
    SkIPoint fOrigin;

    bool fDone, fNeedsTiling;

public:
    static bool NeedsTiling(SkBitmapDevice* dev) {
        return skia_rs_canvas_draw_tiler_needs_tiling(dev->width(), dev->height());
    }

    SkDrawTiler(SkBitmapDevice* dev, const SkRect* bounds) : fDevice(dev) {
        if (!fDevice->accessPixels(&fRootPixmap)) {
            fRootPixmap.reset(fDevice->imageInfo(), nullptr, 0);
        }

        const SkIRect clipR = fDevice->fRCStack.rc().getBounds();
        SkiaRsDrawTilerInit init =
                skia_rs_canvas_draw_tiler_init(clipR, &fDevice->localToDevice(), bounds);
        fNeedsTiling = init.needs_tiling;
        fDone = init.done;
        fSrcBounds = init.src_bounds;
        fOrigin = init.origin;

        if (fNeedsTiling) {
            fDraw.fRC = &fTileRC;
        } else {
            fDraw.fDst = fRootPixmap;
            fDraw.fCTM = &fDevice->localToDevice();
            fDraw.fRC = &fDevice->fRCStack.rc();
        }

        fDraw.fProps = &fDevice->surfaceProps();
    }

    bool needsTiling() const { return fNeedsTiling; }

    const skcpu::Draw* next() {
        if (fDone) {
            return nullptr;
        }
        if (fNeedsTiling) {
            do {
                this->stepAndSetupTileDraw();
            } while (!fDone && fTileRC.isEmpty());
            if (fTileRC.isEmpty()) {
                SkASSERT(fDone);
                return nullptr;
            }
            SkASSERT(!fTileRC.isEmpty());
        } else {
            fDone = true;
        }
        return &fDraw;
    }

private:
    void stepAndSetupTileDraw() {
        SkASSERT(!fDone);
        SkASSERT(fNeedsTiling);

        SkiaRsDrawTilerStep step;
        skia_rs_canvas_draw_tiler_step(fOrigin, fSrcBounds, &fDevice->localToDevice(), &step);
        fOrigin = step.origin;
        fDone = step.done;

        SkASSERT(!step.tile_bounds.isEmpty());
        bool success = fRootPixmap.extractSubset(&fDraw.fDst, step.tile_bounds);
        SkASSERT_RELEASE(success);

        fTileMatrix = step.tile_matrix;
        fDraw.fCTM = &fTileMatrix.value();
        fDevice->fRCStack.rc().translate(-fOrigin.x(), -fOrigin.y(), &fTileRC);
        fTileRC.op(SkIRect::MakeSize(fDraw.fDst.dimensions()), SkClipOp::kIntersect);
    }
};

#define LOOP_TILER(code, boundsPtr)                            \
    SkDrawTiler priv_tiler(this, boundsPtr);                   \
    while (const skcpu::Draw* priv_draw = priv_tiler.next()) { \
        priv_draw->code;                                       \
    }

class SkBitmapDevice::BDDraw : public skcpu::Draw {
public:
    BDDraw(SkBitmapDevice* dev) {
        if (!dev->accessPixels(&fDst)) {
            fDst.reset(dev->imageInfo(), nullptr, 0);
        }
        fCTM = &dev->localToDevice();
        fRC = &dev->fRCStack.rc();
    }
};

static bool valid_for_bitmap_device(const SkImageInfo& info, SkAlphaType* newAlphaType) {
    SkiaRsBitmapDeviceValidation v = skia_rs_canvas_valid_for_bitmap_device(
            info.width(),
            info.height(),
            static_cast<int32_t>(info.colorType()),
            static_cast<int32_t>(info.alphaType()),
            SkColorTypeIsAlwaysOpaque(info.colorType()));
    if (!v.valid) {
        return false;
    }
    if (newAlphaType) {
        *newAlphaType = static_cast<SkAlphaType>(v.new_alpha_type);
    }
    return true;
}

SkBitmapDevice::SkBitmapDevice(const SkBitmap& bitmap)
        : SkBitmapDevice(skcpu::Recorder::TODO(), bitmap) {}

SkBitmapDevice::SkBitmapDevice(const SkBitmap& bitmap,
                               const SkSurfaceProps& surfaceProps,
                               SkRasterHandleAllocator::Handle hndl)
        : SkBitmapDevice(skcpu::Recorder::TODO(), bitmap, surfaceProps, hndl) {}

SkBitmapDevice::SkBitmapDevice(skcpu::Recorder* recorder, const SkBitmap& bitmap)
        : SkDevice(bitmap.info(), SkSurfaceProps())
        , fRecorder(recorder)
        , fBitmap(bitmap)
        , fRCStack(bitmap.width(), bitmap.height())
        , fGlyphPainter(this->surfaceProps(), bitmap.colorType(), bitmap.colorSpace()) {
    SkASSERT(valid_for_bitmap_device(bitmap.info(), nullptr));
}

SkBitmapDevice::SkBitmapDevice(skcpu::Recorder* recorder,
                               const SkBitmap& bitmap,
                               const SkSurfaceProps& surfaceProps,
                               SkRasterHandleAllocator::Handle hndl)
        : SkDevice(bitmap.info(), surfaceProps)
        , fRasterHandle(hndl)
        , fRecorder(recorder)
        , fBitmap(bitmap)
        , fRCStack(bitmap.width(), bitmap.height())
        , fGlyphPainter(this->surfaceProps(), bitmap.colorType(), bitmap.colorSpace()) {
    SkASSERT(valid_for_bitmap_device(bitmap.info(), nullptr));
}

SkRecorder* SkBitmapDevice::baseRecorder() const { return fRecorder; }

sk_sp<SkBitmapDevice> SkBitmapDevice::Create(const SkImageInfo& origInfo,
                                             const SkSurfaceProps& surfaceProps,
                                             SkRasterHandleAllocator* allocator) {
    SkAlphaType newAT = origInfo.alphaType();
    if (!valid_for_bitmap_device(origInfo, &newAT)) {
        return nullptr;
    }

    SkRasterHandleAllocator::Handle hndl = nullptr;
    const SkImageInfo info = origInfo.makeAlphaType(newAT);
    SkBitmap bitmap;

    if (kUnknown_SkColorType == info.colorType()) {
        if (!bitmap.setInfo(info)) {
            return nullptr;
        }
    } else if (allocator) {
        hndl = allocator->allocBitmap(info, &bitmap);
        if (!hndl) {
            return nullptr;
        }
    } else if (info.isOpaque()) {
        if (!bitmap.tryAllocPixels(info)) {
            return nullptr;
        }
    } else {
        if (!bitmap.tryAllocPixelsFlags(info, SkBitmap::kZeroPixels_AllocFlag)) {
            return nullptr;
        }
    }

    return sk_make_sp<SkBitmapDevice>(bitmap, surfaceProps, hndl);
}

void SkBitmapDevice::replaceBitmapBackendForRasterSurface(const SkBitmap& bm) {
    SkASSERT(bm.width() == fBitmap.width());
    SkASSERT(bm.height() == fBitmap.height());
    fBitmap = bm;
}

sk_sp<SkDevice> SkBitmapDevice::createDevice(const CreateInfo& cinfo, const SkPaint* layerPaint) {
    const SkSurfaceProps surfaceProps =
            this->surfaceProps().cloneWithPixelGeometry(cinfo.fPixelGeometry);

    SkImageInfo info = cinfo.fInfo;
    if (layerPaint && layerPaint->getImageFilter()) {
        info = info.makeColorType(kN32_SkColorType);
    }

    return SkBitmapDevice::Create(info, surfaceProps, cinfo.fAllocator);
}

bool SkBitmapDevice::onAccessPixels(SkPixmap* pmap) {
    if (this->onPeekPixels(pmap)) {
        fBitmap.notifyPixelsChanged();
        return true;
    }
    return false;
}

bool SkBitmapDevice::onPeekPixels(SkPixmap* pmap) {
    const SkImageInfo info = fBitmap.info();
    if (fBitmap.getPixels() && (kUnknown_SkColorType != info.colorType())) {
        pmap->reset(fBitmap.info(), fBitmap.getPixels(), fBitmap.rowBytes());
        return true;
    }
    return false;
}

bool SkBitmapDevice::onWritePixels(const SkPixmap& pm, int x, int y) {
    if (nullptr == fBitmap.getPixels()) {
        return false;
    }
    if (fBitmap.writePixels(pm, x, y)) {
        fBitmap.notifyPixelsChanged();
        return true;
    }
    return false;
}

bool SkBitmapDevice::onReadPixels(const SkPixmap& pm, int x, int y) {
    return fBitmap.readPixels(pm, x, y);
}

void SkBitmapDevice::drawPaint(const SkPaint& paint) {
    BDDraw(this).drawPaint(paint);
}

void SkBitmapDevice::drawPoints(SkCanvas::PointMode mode, SkSpan<const SkPoint> pts,
                                const SkPaint& paint) {
    LOOP_TILER(drawPoints(mode, pts, paint, nullptr), nullptr)
}

void SkBitmapDevice::drawRect(const SkRect& r, const SkPaint& paint) {
    LOOP_TILER(drawRect(r, paint), Bounder(r, paint))
}

void SkBitmapDevice::drawOval(const SkRect& oval, const SkPaint& paint) {
    LOOP_TILER(drawOval(oval, paint), Bounder(oval, paint))
}

void SkBitmapDevice::drawRRect(const SkRRect& rrect, const SkPaint& paint) {
    LOOP_TILER(drawRRect(rrect, paint), Bounder(rrect.getBounds(), paint))
}

void SkBitmapDevice::drawPath(const SkPath& path, const SkPaint& paint) {
    const SkRect* bounds = nullptr;
    if (SkDrawTiler::NeedsTiling(this) && !path.isInverseFillType()) {
        bounds = &path.getBounds();
    }
    SkDrawTiler tiler(this, bounds ? Bounder(*bounds, paint).bounds() : nullptr);
    while (const skcpu::Draw* draw = tiler.next()) {
        draw->drawPath(path, paint, nullptr);
    }
}

void SkBitmapDevice::drawBitmap(const SkBitmap& bitmap,
                                const SkMatrix& matrix,
                                const SkRect* dstOrNull,
                                const SkSamplingOptions& sampling,
                                const SkPaint& paint,
                                sk_sp<SkMipmap> mips) {
    const SkRect* bounds = dstOrNull;
    SkRect storage;
    if (!bounds && SkDrawTiler::NeedsTiling(this)) {
        matrix.mapRect(&storage, SkRect::MakeIWH(bitmap.width(), bitmap.height()));
        Bounder b(storage, paint);
        if (b.hasBounds()) {
            storage = *b.bounds();
            bounds = &storage;
        }
    }
    LOOP_TILER(drawBitmap(bitmap, matrix, dstOrNull, sampling, paint, mips), bounds)
}

void SkBitmapDevice::drawImageRect(const SkImage* image, const SkRect* src, const SkRect& dst,
                                   const SkSamplingOptions& sampling, const SkPaint& paint,
                                   SkCanvas::SrcRectConstraint constraint) {
    SkASSERT(image);
    SkASSERT(dst.isFinite());
    SkASSERT(dst.isSorted());

    SkBitmap bitmap;
    auto imageBase = as_IB(image);
    auto dContext = imageBase->directContext();
    if (!imageBase->getROPixels(dContext, &bitmap)) {
        return;
    }
    sk_sp<SkMipmap> mips = imageBase->refMips();

    SkiaRsBitmapImageRectPlan plan;
    skia_rs_canvas_bitmap_image_rect_plan(
            bitmap.width(),
            bitmap.height(),
            src,
            dst,
            SkCanvas::kFast_SrcRectConstraint == constraint,
            sampling == SkSamplingOptions(),
            &plan);

    if (plan.action == 0) {
        return;
    }

    const SkRect* dstPtr = plan.use_tmp_dst ? &plan.dst_rect : &dst;
    const SkBitmap* bitmapPtr = &bitmap;
    SkBitmap tmpBitmap;
    SkMatrix matrix = plan.matrix;

    if (plan.action == 1) {
        goto USE_SHADER;
    }

    if (plan.action == 2) {
        if (!bitmap.extractSubset(&tmpBitmap, plan.src_ir)) {
            return;
        }
        bitmapPtr = &tmpBitmap;
        mips = nullptr;

#ifdef SK_DRAWBITMAPRECT_FAST_OFFSET
        SkScalar dx = plan.src_ir.fLeft > 0 ? SkIntToScalar(plan.src_ir.fLeft) : 0;
        SkScalar dy = plan.src_ir.fTop > 0 ? SkIntToScalar(plan.src_ir.fTop) : 0;
        SkRect extractedBitmapBounds = SkRect::MakeXYWH(dx, dy,
                                                        SkIntToScalar(bitmapPtr->width()),
                                                        SkIntToScalar(bitmapPtr->height()));
#else
        SkRect extractedBitmapBounds;
        extractedBitmapBounds.setIWH(bitmapPtr->width(), bitmapPtr->height());
#endif
        if (extractedBitmapBounds == plan.tmp_src) {
            goto USE_DRAWBITMAP;
        }
    } else {
    USE_DRAWBITMAP:
        if (skia_rs_canvas_can_apply_dst_matrix_as_ctm(&matrix, paint.getMaskFilter() != nullptr)) {
            this->drawBitmap(*bitmapPtr, matrix, dstPtr, sampling, paint, mips);
            return;
        }
    }

USE_SHADER:
    auto img = SkImage_Raster::MakeFromBitmap(*bitmapPtr, SkCopyPixelsMode::kNever, mips);
    auto shader = img->makeShaderForPaint(
            paint, SkTileMode::kClamp, SkTileMode::kClamp, sampling, &matrix);
    if (!shader) {
        return;
    }

    SkPaint paintWithShader(paint);
    paintWithShader.setStyle(SkPaint::kFill_Style);
    paintWithShader.setShader(std::move(shader));

    this->drawRect(*dstPtr, paintWithShader);
}

void SkBitmapDevice::onDrawGlyphRunList(SkCanvas* canvas,
                                        const sktext::GlyphRunList& glyphRunList,
                                        const SkPaint& paint) {
    SkASSERT(!glyphRunList.hasRSXForm());
    LOOP_TILER(drawGlyphRunList(canvas, &fGlyphPainter, glyphRunList, paint), nullptr)
}

void SkBitmapDevice::drawVertices(const SkVertices* vertices,
                                  sk_sp<SkBlender> blender,
                                  const SkPaint& paint,
                                  bool skipColorXform) {
#ifdef SK_LEGACY_IGNORE_DRAW_VERTICES_BLEND_WITH_NO_SHADER
    if (!paint.getShader()) {
        blender = SkBlender::Mode(SkBlendMode::kDst);
    }
#endif
    BDDraw(this).drawVertices(vertices, std::move(blender), paint, skipColorXform);
}

void SkBitmapDevice::drawMesh(const SkMesh&, sk_sp<SkBlender>, const SkPaint&) {}

void SkBitmapDevice::drawAtlas(SkSpan<const SkRSXform> xform,
                               SkSpan<const SkRect> tex,
                               SkSpan<const SkColor> colors,
                               sk_sp<SkBlender> blender,
                               const SkPaint& paint) {
    BDDraw(this).drawAtlas(xform, tex, colors, std::move(blender), paint);
}

void SkBitmapDevice::drawSpecial(SkSpecialImage* src,
                                 const SkMatrix& localToDevice,
                                 const SkSamplingOptions& sampling,
                                 const SkPaint& paint,
                                 SkCanvas::SrcRectConstraint) {
    SkASSERT(!paint.getImageFilter());
    SkASSERT(!paint.getMaskFilter());
    SkASSERT(!src->isGaneshBacked());
    SkASSERT(!src->isGraphiteBacked());

    SkBitmap resultBM;
    if (SkSpecialImages::AsBitmap(src, &resultBM)) {
        skcpu::Draw draw;
        if (!this->accessPixels(&draw.fDst)) {
            return;
        }
        draw.fCTM = &localToDevice;
        draw.fRC = &fRCStack.rc();
        draw.drawBitmap(resultBM, SkMatrix::I(), nullptr, sampling, paint, nullptr);
    }
}

void SkBitmapDevice::drawCoverageMask(const SkSpecialImage* mask,
                                      const SkMatrix& maskToDevice,
                                      const SkSamplingOptions& sampling,
                                      const SkPaint& paint) {
    SkASSERT(!mask->isGaneshBacked());
    SkASSERT(!mask->isGraphiteBacked());

    SkBitmap maskBM;
    if (!SkSpecialImages::AsBitmap(mask, &maskBM)) {
        return;
    }

    skcpu::Draw draw;
    if (!this->accessPixels(&draw.fDst)) {
        return;
    }
    draw.fRC = &fRCStack.rc();
    draw.fCTM = &maskToDevice;
    draw.drawBitmapAsMask(maskBM, sampling, paint, &this->localToDevice());
}

bool SkBitmapDevice::drawBlurredRRect(const SkRRect& rrect, const SkPaint& paint,
                                      SkV2, float) {
    if (!this->localToDevice().isSimilarity()) {
        return false;
    }

    SkASSERT(paint.getMaskFilter() &&
             as_MFB(paint.getMaskFilter())->type() == SkMaskFilterBase::Type::kBlur);

    SkDrawTiler tiler(this, Bounder(rrect.getBounds(), paint));
    if (!tiler.needsTiling()) {
        if (const skcpu::Draw* draw = tiler.next()) {
            return draw->drawRRectNinePatch(rrect, paint);
        }
    }
    return false;
}

sk_sp<SkSpecialImage> SkBitmapDevice::snapSpecial(const SkIRect& bounds, bool forceCopy) {
    if (forceCopy) {
        return SkSpecialImages::CopyFromRaster(bounds, fBitmap, this->surfaceProps());
    } else {
        return SkSpecialImages::MakeFromRaster(bounds, fBitmap, this->surfaceProps());
    }
}

sk_sp<SkSurface> SkBitmapDevice::makeSurface(const SkImageInfo& info, const SkSurfaceProps& props) {
    return SkSurfaces::Raster(info, &props);
}

void SkBitmapDevice::pushClipStack() { fRCStack.save(); }
void SkBitmapDevice::popClipStack() { fRCStack.restore(); }

void SkBitmapDevice::clipRect(const SkRect& rect, SkClipOp op, bool aa) {
    fRCStack.clipRect(this->localToDevice(), rect, op, aa);
}

void SkBitmapDevice::clipRRect(const SkRRect& rrect, SkClipOp op, bool aa) {
    fRCStack.clipRRect(this->localToDevice(), rrect, op, aa);
}

void SkBitmapDevice::clipPath(const SkPath& path, SkClipOp op, bool aa) {
    fRCStack.clipPath(this->localToDevice(), path, op, aa);
}

void SkBitmapDevice::onClipShader(sk_sp<SkShader> sh) {
    fRCStack.clipShader(std::move(sh));
}

void SkBitmapDevice::clipRegion(const SkRegion& rgn, SkClipOp op) {
    SkIPoint origin = this->getOrigin();
    SkRegion tmp;
    const SkRegion* ptr = &rgn;
    if (origin.fX | origin.fY) {
        rgn.translate(-origin.fX, -origin.fY, &tmp);
        ptr = &tmp;
    }
    fRCStack.clipRegion(*ptr, op);
}

void SkBitmapDevice::replaceClip(const SkIRect& rect) {
    SkRect deviceRect = SkMatrixPriv::MapRect(this->globalToDevice(), SkRect::Make(rect));
    fRCStack.replaceClip(deviceRect.round());
}

bool SkBitmapDevice::isClipWideOpen() const {
    const SkRasterClip& rc = fRCStack.rc();
    const bool isBW = rc.isBW();
    return skia_rs_canvas_bitmap_is_clip_wide_open(
            isBW,
            isBW && rc.bwRgn().isRect(),
            isBW ? rc.bwRgn().getBounds() : SkIRect::MakeEmpty(),
            this->width(),
            this->height());
}

bool SkBitmapDevice::isClipEmpty() const {
    return fRCStack.rc().isEmpty();
}

bool SkBitmapDevice::isClipRect() const {
    const SkRasterClip& rc = fRCStack.rc();
    return skia_rs_canvas_bitmap_is_clip_rect(
            rc.isEmpty(), rc.isRect(), SkToBool(rc.clipShader()));
}

bool SkBitmapDevice::isClipAntiAliased() const {
    const SkRasterClip& rc = fRCStack.rc();
    return !rc.isEmpty() && rc.isAA();
}

void SkBitmapDevice::android_utils_clipAsRgn(SkRegion* rgn) const {
    const SkRasterClip& rc = fRCStack.rc();
    if (rc.isAA()) {
        rgn->setRect(rc.getBounds());
    } else {
        *rgn = rc.bwRgn();
    }
}

SkIRect SkBitmapDevice::devClipBounds() const {
    return fRCStack.rc().getBounds();
}

#undef LOOP_TILER

// ============================================================================
// SkClipStackDevice.cpp
// ============================================================================

SkIRect SkClipStackDevice::devClipBounds() const {
    SkIRect r = fClipStack.bounds(this->imageInfo().bounds()).roundOut();
    if (!r.isEmpty()) {
        SkASSERT(this->imageInfo().bounds().contains(r));
    }
    return r;
}

void SkClipStackDevice::pushClipStack() { fClipStack.save(); }
void SkClipStackDevice::popClipStack() { fClipStack.restore(); }

void SkClipStackDevice::clipRect(const SkRect& rect, SkClipOp op, bool aa) {
    fClipStack.clipRect(rect, this->localToDevice(), op, aa);
}

void SkClipStackDevice::clipRRect(const SkRRect& rrect, SkClipOp op, bool aa) {
    fClipStack.clipRRect(rrect, this->localToDevice(), op, aa);
}

void SkClipStackDevice::clipPath(const SkPath& path, SkClipOp op, bool aa) {
    fClipStack.clipPath(path, this->localToDevice(), op, aa);
}

void SkClipStackDevice::onClipShader(sk_sp<SkShader> shader) {
    fClipStack.clipShader(std::move(shader));
}

void SkClipStackDevice::clipRegion(const SkRegion& rgn, SkClipOp op) {
    SkIPoint origin = this->getOrigin();
    SkPathBuilder builder;
    rgn.addBoundaryPath(&builder);
    builder.transform(SkMatrix::Translate(-origin));
    fClipStack.clipPath(builder.detach(), SkMatrix::I(), op, false);
}

void SkClipStackDevice::replaceClip(const SkIRect& rect) {
    SkRect deviceRect = SkMatrixPriv::MapRect(this->globalToDevice(), SkRect::Make(rect));
    fClipStack.replaceClip(deviceRect, /*doAA=*/false);
}

bool SkClipStackDevice::isClipAntiAliased() const {
    SkClipStack::B2TIter iter(fClipStack);
    const SkClipStack::Element* element;
    while ((element = iter.next()) != nullptr) {
        if (element->isAA()) {
            return true;
        }
    }
    return false;
}

bool SkClipStackDevice::isClipWideOpen() const {
    return fClipStack.quickContains(SkRect::MakeIWH(this->width(), this->height()));
}

bool SkClipStackDevice::isClipEmpty() const {
    return fClipStack.isEmpty(SkIRect::MakeWH(this->width(), this->height()));
}

bool SkClipStackDevice::isClipRect() const {
    if (this->isClipWideOpen()) {
        return true;
    } else if (this->isClipEmpty()) {
        return false;
    }

    SkClipStack::BoundsType boundType;
    bool isIntersectionOfRects;
    SkRect bounds;
    fClipStack.getBounds(&bounds, &boundType, &isIntersectionOfRects);
    return isIntersectionOfRects && boundType == SkClipStack::kNormal_BoundsType;
}

void SkClipStackDevice::android_utils_clipAsRgn(SkRegion* rgn) const {
    SkClipStack::BoundsType boundType;
    bool isIntersectionOfRects;
    SkRect bounds;
    fClipStack.getBounds(&bounds, &boundType, &isIntersectionOfRects);
    if (isIntersectionOfRects && SkClipStack::kNormal_BoundsType == boundType) {
        rgn->setRect(bounds.round());
    } else {
        SkRegion boundsRgn({0, 0, this->width(), this->height()});
        *rgn = boundsRgn;
        SkClipStack::B2TIter iter(fClipStack);
        while (auto elem = iter.next()) {
            SkPath tmpPath = elem->asDeviceSpacePath();
            SkRegion tmpRgn;
            tmpRgn.setPath(tmpPath, boundsRgn);
            if (elem->isReplaceOp()) {
                rgn->setRect(elem->getDeviceSpaceRect().round());
            } else {
                rgn->op(tmpRgn, static_cast<SkRegion::Op>(elem->getOp()));
            }
        }
    }
}

// ============================================================================
// src/image/SkTiledImageUtils.cpp
// ============================================================================

namespace SkTiledImageUtils {

void DrawImageRect(SkCanvas* canvas,
                   const SkImage* image,
                   const SkRect& src,
                   const SkRect& dst,
                   const SkSamplingOptions& sampling,
                   const SkPaint* paint,
                   SkCanvas::SrcRectConstraint constraint) {
    if (!image || !canvas) {
        return;
    }

    SkPaint p;
    if (paint) {
        p = *paint;
    }
    if (!SkCanvasPriv::TopDevice(canvas)->drawAsTiledImageRect(
                canvas, image, &src, dst, sampling, p, constraint)) {
        canvas->drawImageRect(image, src, dst, sampling, paint, constraint);
    }
}

void GetImageKeyValues(const SkImage* image, uint32_t keyValues[kNumImageKeyValues]) {
    if (!image || !keyValues) {
        if (keyValues) {
            memset(keyValues, 0, kNumImageKeyValues * sizeof(uint32_t));
        }
        return;
    }

    const SkImage_Base* imageBase = as_IB(image);
    if (const SkBitmap* bm = imageBase->onPeekBitmap()) {
        SkIRect subset = image->bounds();
        SkIPoint origin = bm->pixelRefOrigin();
        skia_rs_canvas_tiled_image_bitmap_keys(
                bm->pixelRef()->getGenerationID(), subset, origin, keyValues);
        return;
    }

    if (imageBase->type() == SkImage_Base::Type::kLazyPicture) {
        const SkImage_Picture* pictureImage = static_cast<const SkImage_Picture*>(imageBase);
        if (pictureImage->getImageKeyValues(keyValues)) {
            return;
        }
    }

    skia_rs_canvas_tiled_image_unique_id_keys(image->uniqueID(), keyValues);
}

}  // namespace SkTiledImageUtils
