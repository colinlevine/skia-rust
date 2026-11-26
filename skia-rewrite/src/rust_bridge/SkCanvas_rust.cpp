/*
 * Safe Rust bridge for:
 *   src/core/SkCanvas.cpp
 *   src/core/SkCanvas_Raster.cpp
 *   src/core/SkCanvasPriv.cpp
 *   src/core/SkOverdrawCanvas.cpp
 * (Phase 1 Layer 5.3 skia-rs-canvas, GN target :core).
 */

#include "include/core/SkAlphaType.h"
#include "include/core/SkBitmap.h"
#include "include/core/SkBlendMode.h"
#include "include/core/SkBlender.h"
#include "include/core/SkBlurTypes.h"
#include "include/core/SkCanvas.h"
#include "include/core/SkColor.h"
#include "include/core/SkColorFilter.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkColorType.h"
#include "include/core/SkDrawable.h"
#include "include/core/SkImage.h"
#include "include/core/SkImageFilter.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkMaskFilter.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkOverdrawCanvas.h"
#include "include/core/SkPath.h"
#include "include/core/SkPathEffect.h"
#include "include/core/SkPicture.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRRect.h"
#include "include/core/SkRSXform.h"
#include "include/core/SkRasterHandleAllocator.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkRegion.h"
#include "include/core/SkShader.h"
#include "include/core/SkStrokeRec.h"
#include "include/core/SkSurface.h"
#include "include/core/SkSurfaceProps.h"
#include "include/core/SkTextBlob.h"
#include "include/core/SkTileMode.h"
#include "include/core/SkTypes.h"
#include "include/core/SkVertices.h"
#include "include/private/SkAlign.h"
#include "include/private/SkAssert.h"
#include "include/private/SkDebug.h"
#include "include/private/SkFloatingPoint.h"
#include "include/private/SkOnlyStackAllocatable.h"
#include "include/private/SkSafe32.h"
#include "include/private/SkTDArray.h"
#include "include/private/SkTPin.h"
#include "include/private/SkTemplates.h"
#include "include/private/SkTo.h"
#include "include/private/chromium/Slug.h"
#include "include/utils/SkNoDrawCanvas.h"
#include "src/core/SkAutoMalloc.h"
#include "src/core/SkBitmapDevice.h"
#include "src/core/SkBlenderBase.h"
#include "src/core/SkBlurMaskFilterImpl.h"
#include "src/core/SkCanvasPriv.h"
#include "src/core/SkDevice.h"
#include "src/core/SkDraw.h"
#include "src/core/SkDrawShadowInfo.h"
#include "src/core/SkGlyph.h"
#include "src/core/SkGlyphRunPainter.h"
#include "src/core/SkImageFilterTypes.h"
#include "src/core/SkImageFilter_Base.h"
#include "src/core/SkLatticeIter.h"
#include "src/core/SkMSAN.h"
#include "src/core/SkMask.h"
#include "src/core/SkMaskFilterBase.h"
#include "src/core/SkMatrixPriv.h"
#include "src/core/SkMipmap.h"
#include "src/core/SkPaintPriv.h"
#include "src/core/SkReadBuffer.h"
#include "src/core/SkSpecialImage.h"
#include "src/core/SkSurfacePriv.h"
#include "src/core/SkTraceEvent.h"
#include "src/core/SkVerticesPriv.h"
#include "src/core/SkWriteBuffer.h"
#include "src/core/SkWriter32.h"
#include "src/core/SkZip.h"
#include "src/effects/colorfilters/SkColorFilterBase.h"
#include "src/image/SkSurface_Base.h"
#include "src/shaders/SkImageShader.h"
#include "src/text/GlyphRun.h"
#include "src/utils/SkPatchUtils.h"

#include <algorithm>
#include <cstdint>
#include <limits>
#include <memory>
#include <new>
#include <optional>
#include <tuple>
#include <utility>

#define RETURN_ON_NULL(ptr)   do { if (nullptr == (ptr)) return; } while (0)
#define RETURN_ON_FALSE(pred) do { if (!(pred)) return; } while (0)

using Slug = sktext::gpu::Slug;

namespace {

struct SkiaRsDstClipMatrixCounts {
    int32_t total_dst_clip_count;
    int32_t total_matrix_count;
};

struct SkiaRsWritePixelsPreflight {
    bool intersects;
    bool complete_overwrite;
};

struct SkiaRsPaddedLayerBoundsResult {
    bool padded;
    SkIRect bounds;
};

struct SkiaRsCleanedSamplingFlags {
    bool changed;
    int32_t mode;
};

static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkIRect) == 16);
static_assert(sizeof(SkISize) == 8);
static_assert(sizeof(SkPoint) == 8);
static_assert(sizeof(SkMatrix) == 40);
static_assert(sizeof(SkM44) == 64);
static_assert(sizeof(SkiaRsDstClipMatrixCounts) == 8);
static_assert(sizeof(SkiaRsWritePixelsPreflight) == 2);
static_assert(sizeof(SkiaRsPaddedLayerBoundsResult) == 20);
static_assert(sizeof(SkiaRsCleanedSamplingFlags) == 8);

extern "C" {
SkPoint skia_rs_canvas_decomposition_center(
        const SkM44* dst_to_local, const SkRect* content_bounds, SkIRect target_output);
int32_t skia_rs_canvas_max_layer_dim(SkIRect target_output);
int32_t skia_rs_canvas_image_filter_color_type(
        int32_t color_type, int32_t bytes_per_pixel, int32_t n32_color_type);
SkiaRsPaddedLayerBoundsResult skia_rs_canvas_padded_layer_bounds(
        SkIRect layer_bounds, int32_t pad);
bool skia_rs_canvas_would_overwrite_check(
        SkISize base_size, bool is_root_device, bool root_clip_wide_open,
        const SkRect* rect, const SkMatrix* total_matrix, bool has_paint,
        int32_t paint_style, bool has_mask_filter, bool has_path_effect, bool has_image_filter);
bool skia_rs_canvas_quick_reject_rect(
        const SkM44* m44, SkRect src, SkRect quick_reject_bounds);
SkRect skia_rs_canvas_compute_device_clip_bounds(
        bool clip_empty, const SkM44* device_to_global, SkIRect dev_clip_bounds,
        bool outset_for_aa);
SkRect skia_rs_canvas_compute_local_clip_bounds(SkIRect device_clip_bounds, const SkM44* ctm);
bool skia_rs_canvas_fillable_rect(SkRect r);
SkiaRsCleanedSamplingFlags skia_rs_canvas_clean_sampling_for_constraint(
        bool is_strict, bool has_mipmap, bool is_aniso);
bool skia_rs_canvas_edge_aa_single_can_map_dst(
        bool has_clip, int32_t matrix_index, const SkMatrix* pre_view_matrix);
int32_t skia_rs_canvas_draw_drrect_action(
        bool outer_empty, bool inner_empty, SkRect outer_bounds, SkRect inner_bounds);
SkRect skia_rs_canvas_draw_circle_bounds(float cx, float cy, float radius);
bool skia_rs_canvas_draw_round_rect_is_rrect(float rx, float ry);
bool skia_rs_canvas_draw_arc_should_skip(SkRect oval, float sweep_angle);
bool skia_rs_canvas_text_blob_add_glyph_count(
        int32_t total, int32_t run_glyph_count, int32_t* out_total);
int32_t skia_rs_canvas_restore_to_count_steps(int32_t save_count, int32_t target_count);
SkiaRsWritePixelsPreflight skia_rs_canvas_write_pixels_preflight(
        int32_t src_w, int32_t src_h, int32_t x, int32_t y, int32_t dev_w, int32_t dev_h);
bool skia_rs_canvas_lattice_validate_flag_count(
        int32_t x_count, int32_t y_count, int32_t flag_count);
size_t skia_rs_canvas_lattice_serialized_size(
        int32_t x_count, int32_t y_count, bool has_rect_types, int32_t* out_flag_count);
SkiaRsDstClipMatrixCounts skia_rs_canvas_dst_clip_and_matrix_counts(
        const uint8_t* has_clips, const int32_t* matrix_indices, int32_t count);
void skia_rs_canvas_overdraw_alpha_matrix(float* out20);
bool skia_rs_canvas_overdraw_is_clear_paint(
        uint32_t color, bool has_color_filter, bool has_shader);
}

}  // namespace

// ============================================================================
// SkCanvas.cpp
// ============================================================================

SK_MAKE_BITMASK_OPS(SkCanvas::PredrawFlags)

bool SkCanvas::wouldOverwriteEntireSurface(const SkRect* rect, const SkPaint* paint,
                                           SkEnumBitMask<PredrawFlags> flags) const {
    auto overrideOpacity = (flags & PredrawFlags::kOpaqueShaderOverride)
                                   ? SkPaintPriv::kOpaque_ShaderOverrideOpacity
                           : (flags & PredrawFlags::kNonOpaqueShaderOverride)
                                   ? SkPaintPriv::kNotOpaque_ShaderOverrideOpacity
                                   : SkPaintPriv::kNone_ShaderOverrideOpacity;

    const SkDevice* root = this->rootDevice();
    const SkDevice* top = this->topDevice();
    const SkMatrix totalMatrix = this->getTotalMatrix();

    if (!skia_rs_canvas_would_overwrite_check(
                this->getBaseLayerSize(),
                root == top,
                root->isClipWideOpen(),
                rect,
                &totalMatrix,
                paint != nullptr,
                paint ? static_cast<int32_t>(paint->getStyle()) : 0,
                paint && paint->getMaskFilter() != nullptr,
                paint && paint->getPathEffect() != nullptr,
                paint && paint->getImageFilter() != nullptr)) {
        return false;
    }
    return SkPaintPriv::Overwrites(paint, overrideOpacity);
}

bool SkCanvas::predrawNotify(bool willOverwritesEntireSurface) {
    if (fSurfaceBase) {
        if (!fSurfaceBase->aboutToDraw(willOverwritesEntireSurface
                                               ? SkSurface::kDiscard_ContentChangeMode
                                               : SkSurface::kRetain_ContentChangeMode)) {
            return false;
        }
    }
    return true;
}

bool SkCanvas::predrawNotify(const SkRect* rect, const SkPaint* paint,
                             SkEnumBitMask<PredrawFlags> flags) {
    if (fSurfaceBase) {
        SkSurface::ContentChangeMode mode = SkSurface::kRetain_ContentChangeMode;
        if (fSurfaceBase->outstandingImageSnapshot()) {
            if (this->wouldOverwriteEntireSurface(rect, paint, flags)) {
                mode = SkSurface::kDiscard_ContentChangeMode;
            }
        }
        if (!fSurfaceBase->aboutToDraw(mode)) {
            return false;
        }
    }
    return true;
}

SkCanvas::Layer::Layer(sk_sp<SkDevice> device,
                       FilterSpan imageFilters,
                       const SkPaint& paint,
                       bool isCoverage,
                       bool includesPadding)
        : fDevice(std::move(device))
        , fImageFilters(imageFilters.data(), imageFilters.size())
        , fPaint(paint)
        , fIsCoverage(isCoverage)
        , fDiscard(false)
        , fIncludesPadding(includesPadding) {
    SkASSERT(fDevice);
    SkASSERT(!fPaint.getImageFilter());
}

SkCanvas::BackImage::BackImage(sk_sp<SkSpecialImage> img, SkIPoint loc)
        : fImage(img), fLoc(loc) {}
SkCanvas::BackImage::BackImage(const BackImage&) = default;
SkCanvas::BackImage::BackImage(BackImage&&) = default;
SkCanvas::BackImage& SkCanvas::BackImage::operator=(const BackImage&) = default;
SkCanvas::BackImage::~BackImage() = default;

SkCanvas::MCRec::MCRec(SkDevice* device) : fDevice(device) {
    SkASSERT(fDevice);
}

SkCanvas::MCRec::MCRec(const MCRec* prev) : fDevice(prev->fDevice), fMatrix(prev->fMatrix) {
    SkASSERT(fDevice);
}

SkCanvas::MCRec::~MCRec() {}

void SkCanvas::MCRec::newLayer(sk_sp<SkDevice> layerDevice,
                               FilterSpan filters,
                               const SkPaint& restorePaint,
                               bool layerIsCoverage,
                               bool includesPadding) {
    SkASSERT(!fBackImage);
    fLayer = std::make_unique<Layer>(std::move(layerDevice),
                                     filters,
                                     restorePaint,
                                     layerIsCoverage,
                                     includesPadding);
    fDevice = fLayer->fDevice.get();
}

void SkCanvas::MCRec::reset(SkDevice* device) {
    SkASSERT(!fLayer);
    SkASSERT(device);
    SkASSERT(fDeferredSaveCount == 0);
    fDevice = device;
    fMatrix.setIdentity();
}

class SkCanvas::AutoUpdateQRBounds {
public:
    SK_ONLY_STACK_ALLOCATABLE;

    explicit AutoUpdateQRBounds(SkCanvas* canvas) : fCanvas(canvas) {
        fCanvas->validateClip();
    }
    ~AutoUpdateQRBounds() {
        fCanvas->fQuickRejectBounds = fCanvas->computeDeviceClipBounds();
        fCanvas->validateClip();
    }

private:
    SkCanvas* fCanvas;
    AutoUpdateQRBounds(AutoUpdateQRBounds&&) = delete;
    AutoUpdateQRBounds(const AutoUpdateQRBounds&) = delete;
    AutoUpdateQRBounds& operator=(AutoUpdateQRBounds&&) = delete;
    AutoUpdateQRBounds& operator=(const AutoUpdateQRBounds&) = delete;
};

std::optional<AutoLayerForImageFilter> SkCanvas::aboutToDraw(
        const SkPaint& paint,
        const SkRect* rawBounds,
        SkEnumBitMask<PredrawFlags> flags) {
    if (flags & PredrawFlags::kCheckForOverwrite) {
        if (!this->predrawNotify(rawBounds, &paint, flags)) {
            return std::nullopt;
        }
    } else {
        if (!this->predrawNotify()) {
            return std::nullopt;
        }
    }

    const bool skipMaskFilterLayer = (flags & PredrawFlags::kSkipMaskFilterAutoLayer) ||
                                     !this->topDevice()->useDrawCoverageMaskForMaskFilters();
    return std::optional<AutoLayerForImageFilter>(
            std::in_place, this, paint, rawBounds, skipMaskFilterLayer);
}

std::optional<AutoLayerForImageFilter> SkCanvas::aboutToDraw(
        const SkPaint& paint,
        const SkRect* rawBounds) {
    return this->aboutToDraw(paint, rawBounds, PredrawFlags::kNone);
}

void SkCanvas::resetForNextPicture(const SkIRect& bounds) {
    this->restoreToCount(1);

    SkASSERT(fRootDevice->isNoPixelsDevice());
    SkNoPixelsDevice* asNoPixelsDevice = static_cast<SkNoPixelsDevice*>(fRootDevice.get());
    if (!asNoPixelsDevice->resetForNextPicture(bounds)) {
        fRootDevice = sk_make_sp<SkNoPixelsDevice>(bounds,
                                                   fRootDevice->surfaceProps(),
                                                   fRootDevice->imageInfo().refColorSpace());
    }

    fMCRec->reset(fRootDevice.get());
    fQuickRejectBounds = this->computeDeviceClipBounds();
}

void SkCanvas::init(sk_sp<SkDevice> device) {
    if (!device) {
        device = sk_make_sp<SkNoPixelsDevice>(SkIRect::MakeEmpty(), fProps);
    }

    SkASSERT(device);

    fSaveCount = 1;
    fMCRec = new (fMCStack.push_back()) MCRec(device.get());

    SkASSERT(fProps.pixelGeometry() == device->surfaceProps().pixelGeometry());

    fSurfaceBase = nullptr;
    fRootDevice = std::move(device);
    fRunBuilders.push_back(std::make_unique<sktext::GlyphRunBuilder>());
    fQuickRejectBounds = this->computeDeviceClipBounds();
}

SkCanvas::SkCanvas() : fMCStack(sizeof(MCRec), fMCRecStorage, sizeof(fMCRecStorage)) {
    this->init(nullptr);
}

SkCanvas::SkCanvas(int width, int height, const SkSurfaceProps* props)
        : fMCStack(sizeof(MCRec), fMCRecStorage, sizeof(fMCRecStorage))
        , fProps(SkSurfacePropsCopyOrDefault(props)) {
    this->init(sk_make_sp<SkNoPixelsDevice>(
            SkIRect::MakeWH(std::max(width, 0), std::max(height, 0)), fProps));
}

SkCanvas::SkCanvas(const SkIRect& bounds)
        : fMCStack(sizeof(MCRec), fMCRecStorage, sizeof(fMCRecStorage)) {
    SkIRect r = bounds.isEmpty() ? SkIRect::MakeEmpty() : bounds;
    this->init(sk_make_sp<SkNoPixelsDevice>(r, fProps));
}

SkCanvas::SkCanvas(sk_sp<SkDevice> device)
        : fMCStack(sizeof(MCRec), fMCRecStorage, sizeof(fMCRecStorage))
        , fProps(device->surfaceProps()) {
    this->init(std::move(device));
}

SkCanvas::~SkCanvas() {
    SkDeque::Iter iter(fMCStack, SkDeque::Iter::kFront_IterStart);
    for (;;) {
        MCRec* rec = (MCRec*)iter.next();
        if (!rec) {
            break;
        }
        if (rec->fLayer) {
            rec->fLayer->fDiscard = true;
        }
    }

    this->restoreToCount(1);
    this->internalRestore();
}

SkSurface* SkCanvas::getSurface() const { return fSurfaceBase; }
SkISize SkCanvas::getBaseLayerSize() const { return this->rootDevice()->imageInfo().dimensions(); }
SkDevice* SkCanvas::topDevice() const {
    SkASSERT(fMCRec->fDevice);
    return fMCRec->fDevice;
}

bool SkCanvas::readPixels(const SkPixmap& pm, int x, int y) {
    return pm.addr() && this->rootDevice()->readPixels(pm, x, y);
}

bool SkCanvas::readPixels(const SkImageInfo& dstInfo, void* dstP, size_t rowBytes, int x, int y) {
    return this->readPixels({dstInfo, dstP, rowBytes}, x, y);
}

bool SkCanvas::readPixels(const SkBitmap& bm, int x, int y) {
    SkPixmap pm;
    return bm.peekPixels(&pm) && this->readPixels(pm, x, y);
}

bool SkCanvas::writePixels(const SkBitmap& bitmap, int x, int y) {
    SkPixmap pm;
    if (bitmap.peekPixels(&pm)) {
        return this->writePixels(pm.info(), pm.addr(), pm.rowBytes(), x, y);
    }
    return false;
}

bool SkCanvas::writePixels(const SkImageInfo& srcInfo, const void* pixels, size_t rowBytes,
                           int x, int y) {
    SkDevice* device = this->rootDevice();
    SkiaRsWritePixelsPreflight pf = skia_rs_canvas_write_pixels_preflight(
            srcInfo.width(), srcInfo.height(), x, y, device->width(), device->height());
    if (!pf.intersects) {
        return false;
    }
    if (!this->predrawNotify(pf.complete_overwrite)) {
        return false;
    }
    return device->writePixels({srcInfo, pixels, rowBytes}, x, y);
}

void SkCanvas::checkForDeferredSave() {
    if (fMCRec->fDeferredSaveCount > 0) {
        this->doSave();
    }
}

int SkCanvas::getSaveCount() const {
#ifdef SK_DEBUG
    int count = 0;
    SkDeque::Iter iter(fMCStack, SkDeque::Iter::kFront_IterStart);
    for (;;) {
        const MCRec* rec = (const MCRec*)iter.next();
        if (!rec) {
            break;
        }
        count += 1 + rec->fDeferredSaveCount;
    }
    SkASSERT(count == fSaveCount);
#endif
    return fSaveCount;
}

int SkCanvas::save() {
    fSaveCount += 1;
    fMCRec->fDeferredSaveCount += 1;
    return this->getSaveCount() - 1;
}

void SkCanvas::doSave() {
    this->willSave();
    SkASSERT(fMCRec->fDeferredSaveCount > 0);
    fMCRec->fDeferredSaveCount -= 1;
    this->internalSave();
}

void SkCanvas::restore() {
    if (fMCRec->fDeferredSaveCount > 0) {
        SkASSERT(fSaveCount > 1);
        fSaveCount -= 1;
        fMCRec->fDeferredSaveCount -= 1;
    } else {
        if (fMCStack.count() > 1) {
            this->willRestore();
            SkASSERT(fSaveCount > 1);
            fSaveCount -= 1;
            this->internalRestore();
            this->didRestore();
        }
    }
}

void SkCanvas::restoreToCount(int count) {
    int n = skia_rs_canvas_restore_to_count_steps(this->getSaveCount(), count);
    for (int i = 0; i < n; ++i) {
        this->restore();
    }
}

void SkCanvas::internalSave() {
    fMCRec = new (fMCStack.push_back()) MCRec(fMCRec);
    this->topDevice()->pushClipStack();
}

int SkCanvas::saveLayer(const SkRect* bounds, const SkPaint* paint) {
    return this->saveLayer(SaveLayerRec(bounds, paint, 0));
}

int SkCanvas::saveLayer(const SaveLayerRec& rec) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    if (rec.fPaint && this->nothingToDraw(*rec.fPaint)) {
        this->save();
        this->clipRect({0, 0, 0, 0});
    } else {
        SaveLayerStrategy strategy = this->getSaveLayerStrategy(rec);
        fSaveCount += 1;
        this->internalSaveLayer(rec, strategy);
    }
    return this->getSaveCount() - 1;
}

int SkCanvas::only_axis_aligned_saveBehind(const SkRect* bounds) {
    if (bounds && !this->getLocalClipBounds().intersects(*bounds)) {
        this->save();
    } else {
        bool doTheWork = this->onDoSaveBehind(bounds);
        fSaveCount += 1;
        this->internalSave();
        if (doTheWork) {
            this->internalSaveBehind(bounds);
        }
    }
    return this->getSaveCount() - 1;
}

static skif::ParameterSpace<SkPoint> compute_decomposition_center(
        const SkM44& dstToLocal,
        std::optional<skif::ParameterSpace<SkRect>> contentBounds,
        const skif::DeviceSpace<SkIRect>& targetOutput) {
    SkRect cb;
    const SkRect* cbPtr = nullptr;
    if (contentBounds) {
        cb = SkRect(*contentBounds);
        cbPtr = &cb;
    }
    SkPoint center = skia_rs_canvas_decomposition_center(
            &dstToLocal, cbPtr, SkIRect(targetOutput));
    return skif::ParameterSpace<SkPoint>(center);
}

struct FilterToSpan {
    FilterToSpan(const SkImageFilter* filter) : fFilter(sk_ref_sp(filter)) {}
    operator SkCanvas::FilterSpan() {
        return fFilter ? SkCanvas::FilterSpan{&fFilter, 1} : SkCanvas::FilterSpan{};
    }
    sk_sp<SkImageFilter> fFilter;
};

static std::optional<std::pair<skif::Mapping, skif::LayerSpace<SkIRect>>>
get_layer_mapping_and_bounds(
        SkCanvas::FilterSpan filters,
        const SkM44& localToDst,
        const skif::DeviceSpace<SkIRect>& targetOutput,
        std::optional<skif::ParameterSpace<SkRect>> contentBounds = {},
        SkScalar scaleFactor = 1.0f) {
    SkM44 dstToLocal;
    if (!localToDst.isFinite() || !localToDst.invert(&dstToLocal)) {
        return {};
    }

    skif::ParameterSpace<SkPoint> center =
            compute_decomposition_center(dstToLocal, contentBounds, targetOutput);

    skif::Mapping mapping;
    skif::MatrixCapability capability = skif::MatrixCapability::kComplex;
    for (const sk_sp<SkImageFilter>& filter : filters) {
        if (filter) {
            capability = std::min(capability, as_IFB(filter)->getCTMCapability());
        }
    }
    if (!mapping.decomposeCTM(localToDst, capability, center)) {
        return {};
    }
    if (scaleFactor != 1.0f &&
        !mapping.adjustLayerSpace(SkM44::Scale(scaleFactor, scaleFactor))) {
        return {};
    }

    int maxLayerDim = skia_rs_canvas_max_layer_dim(SkIRect(targetOutput));

    auto baseLayerBounds = mapping.deviceToLayer(targetOutput);
    if (contentBounds) {
        skif::LayerSpace<SkIRect> knownBounds = mapping.paramToLayer(*contentBounds).roundOut();
        if (!baseLayerBounds.intersect(knownBounds)) {
            baseLayerBounds = skif::LayerSpace<SkIRect>::Empty();
        }
    }

    skif::LayerSpace<SkIRect> layerBounds;
    if (!filters.empty()) {
        layerBounds = skif::LayerSpace<SkIRect>::Union(filters.size(), [&](int i) {
            return filters[i] ? as_IFB(filters[i])
                                        ->getInputBounds(mapping, targetOutput, contentBounds)
                              : baseLayerBounds;
        });
        if (layerBounds.width() > maxLayerDim || layerBounds.height() > maxLayerDim) {
            skif::Mapping idealMapping{mapping.layerMatrix()};
            for (const sk_sp<SkImageFilter>& filter : filters) {
                if (filter) {
                    auto idealLayerBounds = as_IFB(filter)->getInputBounds(
                            idealMapping, targetOutput, contentBounds);
                    maxLayerDim = std::max(
                            std::max(idealLayerBounds.width(), idealLayerBounds.height()),
                            maxLayerDim);
                }
            }
        }
    } else {
        if (baseLayerBounds.isEmpty()) {
            return {};
        }
        layerBounds = baseLayerBounds;
    }

    if (layerBounds.width() > maxLayerDim || layerBounds.height() > maxLayerDim) {
        skif::LayerSpace<SkIRect> newLayerBounds(
                SkIRect::MakeWH(std::min(layerBounds.width(), maxLayerDim),
                                std::min(layerBounds.height(), maxLayerDim)));
        SkM44 adjust = SkM44::RectToRect(SkRect::Make(SkIRect(layerBounds)),
                                         SkRect::Make(SkIRect(newLayerBounds)));
        if (!mapping.adjustLayerSpace(adjust)) {
            return {};
        } else {
            layerBounds = newLayerBounds;
        }
    }

    return std::make_pair(mapping, layerBounds);
}

static SkColorType image_filter_color_type(const SkColorInfo& dstInfo) {
    return static_cast<SkColorType>(skia_rs_canvas_image_filter_color_type(
            static_cast<int32_t>(dstInfo.colorType()),
            dstInfo.bytesPerPixel(),
            static_cast<int32_t>(kN32_SkColorType)));
}

static skif::FilterResult apply_alpha_and_colorfilter(const skif::Context& ctx,
                                                      const skif::FilterResult& image,
                                                      const SkPaint& paint) {
    skif::FilterResult result = image;
    if (paint.getAlphaf() < 1.f) {
        result = result.applyColorFilter(ctx, SkColorFilters::Blend(paint.getColor4f(),
                                                                    /*colorSpace=*/nullptr,
                                                                    SkBlendMode::kDstIn));
    }
    if (paint.getColorFilter()) {
        result = result.applyColorFilter(ctx, paint.refColorFilter());
    }
    return result;
}

void SkCanvas::internalDrawDeviceWithFilter(SkDevice* src,
                                            SkDevice* dst,
                                            FilterSpan filters,
                                            const SkPaint& paint,
                                            DeviceCompatibleWithFilter compat,
                                            const SkColorInfo& filterColorInfo,
                                            SkScalar scaleFactor,
                                            SkTileMode srcTileMode,
                                            bool srcIsCoverageLayer) {
    SkASSERT(dst);

    sk_sp<SkColorSpace> filterColorSpace = filterColorInfo.refColorSpace();

    const SkColorType filterColorType =
            srcIsCoverageLayer ? kAlpha_8_SkColorType : image_filter_color_type(filterColorInfo);

    SkM44 localToSrc = src ? (src->globalToDevice() * fMCRec->fMatrix) : SkM44();
    SkISize srcDims = src ? src->imageInfo().dimensions() : SkISize::Make(0, 0);

    skif::LayerSpace<SkMatrix> srcToLayer;

    skif::Mapping mapping;
    skif::LayerSpace<SkIRect> requiredInput;
    skif::DeviceSpace<SkIRect> outputBounds{dst->devClipBounds()};
    if (compat != DeviceCompatibleWithFilter::kUnknown) {
        SkASSERT(src);
        SkASSERT(scaleFactor == 1.0f);
        SkASSERT(!srcDims.isEmpty());

        mapping = skif::Mapping(src->getRelativeTransform(*dst),
                                dst->getRelativeTransform(*src),
                                localToSrc);
        requiredInput = skif::LayerSpace<SkIRect>(SkIRect::MakeSize(srcDims));
        srcToLayer = skif::LayerSpace<SkMatrix>(SkMatrix::I());
    } else {
        auto mappingAndBounds = get_layer_mapping_and_bounds(
                filters, dst->localToDevice44(), outputBounds, {}, SkTPin(scaleFactor, 0.f, 1.f));
        if (!mappingAndBounds) {
            return;
        }

        std::tie(mapping, requiredInput) = *mappingAndBounds;
        if (src) {
            if (!requiredInput.isEmpty()) {
                SkM44 srcToLocal;
                if (!localToSrc.invert(&srcToLocal)) {
                    return;
                }
                srcToLayer =
                        skif::LayerSpace<SkMatrix>((mapping.layerMatrix() * srcToLocal).asM33());
            }
        } else {
            requiredInput = skif::LayerSpace<SkIRect>::Empty();
        }
    }

    auto backend = dst->createImageFilteringBackend(src ? src->surfaceProps() : dst->surfaceProps(),
                                                    filterColorType);
    skif::Stats stats;
    skif::Context ctx{std::move(backend),
                      mapping,
                      requiredInput,
                      skif::FilterResult{},
                      filterColorSpace.get(),
                      &stats};

    skif::FilterResult source;
    if (src && !requiredInput.isEmpty()) {
        skif::LayerSpace<SkIRect> srcSubset;
        if (!srcToLayer.inverseMapRect(requiredInput, &srcSubset)) {
            return;
        }

        ctx.markNewSurface();

        auto availSrc =
                skif::LayerSpace<SkIRect>(src->size()).relevantSubset(srcSubset, srcTileMode);

        if (SkMatrix(srcToLayer).isScaleTranslate()) {
            auto requiredSubset = srcToLayer.mapRect(availSrc);
            if (requiredSubset.width() == availSrc.width() &&
                requiredSubset.height() == availSrc.height()) {
                source = {src->snapSpecial(SkIRect(availSrc)), requiredSubset.topLeft()};
            } else {
                SkASSERT(compat == DeviceCompatibleWithFilter::kUnknown);
                source = {src->snapSpecialScaled(SkIRect(availSrc),
                                                 SkISize(requiredSubset.size())),
                          requiredSubset.topLeft()};
                ctx.markNewSurface();
            }
        }

        if (compat == DeviceCompatibleWithFilter::kYesWithPadding) {
            SkASSERT(!filters.empty());
            source = source.insetForSaveLayer();
        } else if (compat == DeviceCompatibleWithFilter::kYes) {
            // Leave source as-is.
        } else if (source) {
            source = source.applyCrop(ctx, source.layerBounds(), srcTileMode);
        } else if (!requiredInput.isEmpty()) {
            source = {src->snapSpecial(SkIRect(availSrc)), availSrc.topLeft()};
            source = source.applyCrop(ctx.withNewDesiredOutput(srcSubset),
                                      source.layerBounds(),
                                      srcTileMode)
                             .applyTransform(ctx, srcToLayer, SkFilterMode::kLinear);
        }
    }

    ctx = ctx.withNewDesiredOutput(mapping.deviceToLayer(outputBounds)).withNewSource(source);

    sk_sp<SkImageFilter> nullFilter;
    FilterSpan filtersOrNull = filters.empty() ? FilterSpan{&nullFilter, 1} : filters;

    for (const sk_sp<SkImageFilter>& filter : filtersOrNull) {
        auto result = filter ? as_IFB(filter)->filterImage(ctx) : source;

        if (srcIsCoverageLayer) {
            SkASSERT(dst->useDrawCoverageMaskForMaskFilters());
            auto [coverageMask, origin] = result.imageAndOffset(ctx);
            if (coverageMask) {
                SkM44 deviceMatrixWithOffset = mapping.layerToDevice();
                deviceMatrixWithOffset.preTranslate(origin.x(), origin.y());
                dst->drawCoverageMask(
                        coverageMask.get(), deviceMatrixWithOffset.asM33(),
                        result.sampling(), paint);
            }
        } else {
            result = apply_alpha_and_colorfilter(ctx, result, paint);
            result.draw(ctx, dst, paint.getBlender());
        }
    }

    stats.reportStats();
}

void SkCanvas::internalSaveLayer(const SaveLayerRec& rec,
                                 SaveLayerStrategy strategy,
                                 bool coverageOnly) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    this->internalSave();

    if (this->isClipEmpty()) {
        return;
    }

    SkPaint restorePaint(rec.fPaint ? *rec.fPaint : SkPaint());
    restorePaint.setStyle(SkPaint::kFill_Style);
    restorePaint.setPathEffect(nullptr);
    restorePaint.setMaskFilter(nullptr);
    restorePaint.setImageFilter(nullptr);
    restorePaint.setAntiAlias(true);

    sk_sp<SkImageFilter> paintFilter = rec.fPaint ? rec.fPaint->refImageFilter() : nullptr;
    FilterSpan filters = paintFilter ? FilterSpan{&paintFilter, 1} : rec.fFilters;
    if (filters.size() > kMaxFiltersPerLayer) {
        filters = filters.first(kMaxFiltersPerLayer);
    }
    const SkColorFilter* cf = restorePaint.getColorFilter();
    const SkBlender* blender = restorePaint.getBlender();

    bool filtersPriorDevice = rec.fBackdrop;
#if !defined(SK_LEGACY_INITWITHPREV_LAYER_SIZING)
    filtersPriorDevice |= ((rec.fSaveLayerFlags & kInitWithPrevious_SaveLayerFlag) &&
                           (!filters.empty() || cf || blender || restorePaint.getAlphaf() < 1.f));
#endif
    const bool drawDeviceMustFillClip = filters.empty() &&
                                        ((cf && as_CFB(cf)->affectsTransparentBlack()) ||
                                         (blender && as_BB(blender)->affectsTransparentBlack()));
    const bool trivialRestore = !filtersPriorDevice && !drawDeviceMustFillClip;

    SkDevice* priorDevice = this->topDevice();
    skif::Mapping newLayerMapping;
    skif::LayerSpace<SkIRect> layerBounds;
    skif::DeviceSpace<SkIRect> outputBounds{priorDevice->devClipBounds()};

    std::optional<skif::ParameterSpace<SkRect>> contentBounds;
    if (rec.fBounds && trivialRestore) {
        contentBounds = skif::ParameterSpace<SkRect>(*rec.fBounds);
    }

    auto mappingAndBounds = get_layer_mapping_and_bounds(
            filters, priorDevice->localToDevice44(), outputBounds, contentBounds);

    auto abortLayer = [this]() {
        AutoUpdateQRBounds aqr(this);
        this->topDevice()->clipRect(SkRect::MakeEmpty(), SkClipOp::kIntersect, /*aa=*/false);
    };

    if (!mappingAndBounds) {
        abortLayer();
        return;
    }

    std::tie(newLayerMapping, layerBounds) = *mappingAndBounds;

    bool paddedLayer = false;
    if (layerBounds.isEmpty()) {
        if (!filters.empty() && !priorDevice->isNoPixelsDevice() && this->predrawNotify()) {
            SkColorInfo filterColorInfo = priorDevice->imageInfo().colorInfo();
            if (rec.fColorSpace) {
                filterColorInfo = filterColorInfo.makeColorSpace(sk_ref_sp(rec.fColorSpace));
            }
            this->internalDrawDeviceWithFilter(/*src=*/nullptr, priorDevice, filters, restorePaint,
                                               DeviceCompatibleWithFilter::kUnknown,
                                               filterColorInfo);
        }
        abortLayer();
        return;
    } else if (!filters.empty()) {
        SkiaRsPaddedLayerBoundsResult padRes =
                skia_rs_canvas_padded_layer_bounds(SkIRect(layerBounds), skif::kLayerPadding);
        if (padRes.padded) {
            layerBounds = skif::LayerSpace<SkIRect>(padRes.bounds);
            paddedLayer = true;
        }
    }

    sk_sp<SkDevice> newDevice;
    if (strategy == kFullLayer_SaveLayerStrategy) {
        SkASSERT(!layerBounds.isEmpty());

        SkColorType layerColorType;
        if (coverageOnly) {
            layerColorType = kAlpha_8_SkColorType;
        } else {
            layerColorType = SkToBool(rec.fSaveLayerFlags & kF16ColorType)
                                     ? kRGBA_F16_SkColorType
                                     : image_filter_color_type(priorDevice->imageInfo().colorInfo());
        }
        SkImageInfo info =
                SkImageInfo::Make(layerBounds.width(),
                                  layerBounds.height(),
                                  layerColorType,
                                  kPremul_SkAlphaType,
                                  rec.fColorSpace ? sk_ref_sp(rec.fColorSpace)
                                                  : priorDevice->imageInfo().refColorSpace());

        SkPixelGeometry geo = rec.fSaveLayerFlags & kPreserveLCDText_SaveLayerFlag
                                      ? fProps.pixelGeometry()
                                      : kUnknown_SkPixelGeometry;
        const auto createInfo = SkDevice::CreateInfo(info, geo, fAllocator.get());
        newDevice = priorDevice->createDevice(createInfo, rec.fPaint);
    }

    bool initBackdrop = (rec.fSaveLayerFlags & kInitWithPrevious_SaveLayerFlag) || rec.fBackdrop;
    if (!newDevice) {
        newDevice = sk_make_sp<SkNoPixelsDevice>(SkIRect::MakeWH(layerBounds.width(),
                                                                 layerBounds.height()),
                                                 fProps, this->imageInfo().refColorSpace());
        initBackdrop = false;
    }

    if (paddedLayer) {
        newDevice->clipRect(SkRect::Make(newDevice->devClipBounds().makeInset(skif::kLayerPadding,
                                                                              skif::kLayerPadding)),
                            SkClipOp::kIntersect, /*aa=*/false);
    }

    newDevice->setDeviceCoordinateSystem(
            priorDevice->deviceToGlobal() * newLayerMapping.layerToDevice(),
            newLayerMapping.deviceToLayer() * priorDevice->globalToDevice(),
            newLayerMapping.layerMatrix(),
            layerBounds.left(),
            layerBounds.top());

    if (initBackdrop) {
        SkASSERT(!coverageOnly);
        SkPaint backdropPaint;
        FilterToSpan backdropAsSpan(rec.fBackdrop);
        bool scaleBackdrop = rec.fExperimentalBackdropScale != 1.0f;
        auto compat = (!filters.empty() || rec.fBackdrop || scaleBackdrop)
                              ? DeviceCompatibleWithFilter::kUnknown
                              : DeviceCompatibleWithFilter::kYes;
        this->internalDrawDeviceWithFilter(priorDevice,
                                           newDevice.get(),
                                           backdropAsSpan,
                                           backdropPaint,
                                           compat,
                                           newDevice->imageInfo().colorInfo(),
                                           rec.fExperimentalBackdropScale,
                                           rec.fBackdropTileMode);
    }

    fMCRec->newLayer(std::move(newDevice), filters, restorePaint, coverageOnly, paddedLayer);
    fQuickRejectBounds = this->computeDeviceClipBounds();
}

int SkCanvas::saveLayerAlphaf(const SkRect* bounds, float alpha) {
    if (alpha >= 1.0f) {
        return this->saveLayer(bounds, nullptr);
    } else {
        SkPaint tmpPaint;
        tmpPaint.setAlphaf(alpha);
        return this->saveLayer(bounds, &tmpPaint);
    }
}

void SkCanvas::internalSaveBehind(const SkRect* localBounds) {
    SkDevice* device = this->topDevice();

    SkIRect devBounds;
    if (localBounds) {
        SkRect tmp;
        device->localToDevice().mapRect(&tmp, *localBounds);
        if (!devBounds.intersect(tmp.round(), device->devClipBounds())) {
            devBounds.setEmpty();
        }
    } else {
        devBounds = device->devClipBounds();
    }
    if (devBounds.isEmpty()) {
        return;
    }

    auto backImage = device->snapSpecial(devBounds, /*forceCopy=*/true);
    if (!backImage) {
        return;
    }

    this->checkForDeferredSave();

    fMCRec->fBackImage =
            std::make_unique<BackImage>(BackImage{std::move(backImage), devBounds.topLeft()});

    SkPaint paint;
    paint.setBlendMode(SkBlendMode::kClear);
    this->drawClippedToSaveBehind(paint);
}

void SkCanvas::internalRestore() {
    SkASSERT(!fMCStack.empty());

    std::unique_ptr<Layer> layer = std::move(fMCRec->fLayer);
    std::unique_ptr<BackImage> backImage = std::move(fMCRec->fBackImage);

    fMCRec->~MCRec();
    fMCStack.pop_back();
    fMCRec = (MCRec*)fMCStack.back();

    if (!fMCRec) {
        return;
    }

    this->topDevice()->popClipStack();
    this->topDevice()->setGlobalCTM(fMCRec->fMatrix);

    if (backImage && this->predrawNotify()) {
        SkPaint paint;
        paint.setBlendMode(SkBlendMode::kDstOver);
        this->topDevice()->drawSpecial(backImage->fImage.get(),
                                       SkMatrix::Translate(backImage->fLoc),
                                       SkSamplingOptions(),
                                       paint);
    }

    if (layer && !layer->fDevice->isNoPixelsDevice() && !layer->fDiscard) {
        layer->fDevice->setImmutable();

        if (this->predrawNotify()) {
            SkDevice* dstDev = this->topDevice();
            if (!layer->fImageFilters.empty()) {
                auto compat = layer->fIncludesPadding ? DeviceCompatibleWithFilter::kYesWithPadding
                                                      : DeviceCompatibleWithFilter::kYes;
                this->internalDrawDeviceWithFilter(layer->fDevice.get(),
                                                   dstDev,
                                                   layer->fImageFilters,
                                                   layer->fPaint,
                                                   compat,
                                                   layer->fDevice->imageInfo().colorInfo(),
                                                   /*scaleFactor=*/1.0f,
                                                   /*srcTileMode=*/SkTileMode::kDecal,
                                                   layer->fIsCoverage);
            } else {
                SkASSERT(!layer->fIsCoverage && !layer->fIncludesPadding);
                SkSamplingOptions sampling;
                dstDev->drawDevice(layer->fDevice.get(), sampling, layer->fPaint);
            }
        }
    }

    if (this->getSaveCount() < fClipRestrictionSaveCount) {
        fClipRestrictionRect.setEmpty();
        fClipRestrictionSaveCount = -1;
    }
    fQuickRejectBounds = this->computeDeviceClipBounds();
    this->validateClip();
}

sk_sp<SkSurface> SkCanvas::makeSurface(const SkImageInfo& info, const SkSurfaceProps* props) {
    if (nullptr == props) {
        props = &fProps;
    }
    return this->onNewSurface(info, *props);
}

sk_sp<SkSurface> SkCanvas::onNewSurface(const SkImageInfo& info, const SkSurfaceProps& props) {
    return this->rootDevice()->makeSurface(info, props);
}

SkImageInfo SkCanvas::imageInfo() const { return this->onImageInfo(); }
SkImageInfo SkCanvas::onImageInfo() const { return this->rootDevice()->imageInfo(); }

bool SkCanvas::getProps(SkSurfaceProps* props) const {
    return this->onGetProps(props, /*top=*/false);
}

SkSurfaceProps SkCanvas::getBaseProps() const {
    SkSurfaceProps props;
    this->onGetProps(&props, /*top=*/false);
    return props;
}

SkSurfaceProps SkCanvas::getTopProps() const {
    SkSurfaceProps props;
    this->onGetProps(&props, /*top=*/true);
    return props;
}

bool SkCanvas::onGetProps(SkSurfaceProps* props, bool top) const {
    if (props) {
        *props = top ? topDevice()->surfaceProps() : fProps;
    }
    return true;
}

bool SkCanvas::peekPixels(SkPixmap* pmap) { return this->onPeekPixels(pmap); }
bool SkCanvas::onPeekPixels(SkPixmap* pmap) { return this->rootDevice()->peekPixels(pmap); }

void* SkCanvas::accessTopLayerPixels(SkImageInfo* info, size_t* rowBytes, SkIPoint* origin) {
    SkPixmap pmap;
    if (!this->onAccessTopLayerPixels(&pmap)) {
        return nullptr;
    }
    if (info) {
        *info = pmap.info();
    }
    if (rowBytes) {
        *rowBytes = pmap.rowBytes();
    }
    if (origin) {
        if (this->topDevice()->isPixelAlignedToGlobal()) {
            *origin = this->topDevice()->getOrigin();
        } else {
            return nullptr;
        }
    }
    return pmap.writable_addr();
}

bool SkCanvas::onAccessTopLayerPixels(SkPixmap* pmap) {
    SkPixmap temp;
    if (!this->topDevice()->peekPixels(&temp)) {
        return false;
    }
    if (this->topDevice() == this->rootDevice() && !this->predrawNotify()) {
        return false;
    }
    return this->topDevice()->accessPixels(pmap);
}

void SkCanvas::translate(SkScalar dx, SkScalar dy) {
    if (dx || dy) {
        this->checkForDeferredSave();
        fMCRec->fMatrix.preTranslate(dx, dy);
        this->topDevice()->setGlobalCTM(fMCRec->fMatrix);
        this->didTranslate(dx, dy);
    }
}

void SkCanvas::scale(SkScalar sx, SkScalar sy) {
    if (sx != 1 || sy != 1) {
        this->checkForDeferredSave();
        fMCRec->fMatrix.preScale(sx, sy);
        this->topDevice()->setGlobalCTM(fMCRec->fMatrix);
        this->didScale(sx, sy);
    }
}

void SkCanvas::rotate(SkScalar degrees) {
    SkMatrix m;
    m.setRotate(degrees);
    this->concat(m);
}

void SkCanvas::rotate(SkScalar degrees, SkScalar px, SkScalar py) {
    SkMatrix m;
    m.setRotate(degrees, px, py);
    this->concat(m);
}

void SkCanvas::skew(SkScalar sx, SkScalar sy) {
    SkMatrix m;
    m.setSkew(sx, sy);
    this->concat(m);
}

void SkCanvas::concat(const SkMatrix& matrix) {
    if (matrix.isIdentity()) {
        return;
    }
    this->concat(SkM44(matrix));
}

void SkCanvas::internalConcat44(const SkM44& m) {
    this->checkForDeferredSave();
    fMCRec->fMatrix.preConcat(m);
    this->topDevice()->setGlobalCTM(fMCRec->fMatrix);
}

void SkCanvas::concat(const SkM44& m) {
    this->internalConcat44(m);
    this->didConcat44(m);
}

void SkCanvas::internalSetMatrix(const SkM44& m) {
    fMCRec->fMatrix = m;
    this->topDevice()->setGlobalCTM(fMCRec->fMatrix);
}

void SkCanvas::setMatrix(const SkMatrix& matrix) { this->setMatrix(SkM44(matrix)); }

void SkCanvas::setMatrix(const SkM44& m) {
    this->checkForDeferredSave();
    this->internalSetMatrix(m);
    this->didSetM44(m);
}

void SkCanvas::resetMatrix() { this->setMatrix(SkM44()); }

void SkCanvas::clipRect(const SkRect& rect, SkClipOp op, bool doAA) {
    if (!rect.isFinite()) {
        return;
    }
    this->checkForDeferredSave();
    ClipEdgeStyle edgeStyle = doAA ? kSoft_ClipEdgeStyle : kHard_ClipEdgeStyle;
    this->onClipRect(rect.makeSorted(), op, edgeStyle);
}

void SkCanvas::onClipRect(const SkRect& rect, SkClipOp op, ClipEdgeStyle edgeStyle) {
    SkASSERT(rect.isSorted());
    const bool isAA = kSoft_ClipEdgeStyle == edgeStyle;
    AutoUpdateQRBounds aqr(this);
    this->topDevice()->clipRect(rect, op, isAA);
}

void SkCanvas::androidFramework_setDeviceClipRestriction(const SkIRect& rect) {
    SkASSERT(this->topDevice() == this->rootDevice());
    SkASSERT(fClipRestrictionSaveCount < 0 && fClipRestrictionRect.isEmpty());

    if (fClipRestrictionSaveCount < 0 && !rect.isEmpty()) {
        fClipRestrictionRect = rect;
        fClipRestrictionSaveCount = this->getSaveCount();
        this->checkForDeferredSave();
        AutoUpdateQRBounds aqr(this);
        this->topDevice()->clipRegion(SkRegion(rect), SkClipOp::kIntersect);
    }
}

void SkCanvas::internal_private_resetClip() {
    this->checkForDeferredSave();
    this->onResetClip();
}

void SkCanvas::onResetClip() {
    SkIRect deviceRestriction = this->topDevice()->imageInfo().bounds();
    if (fClipRestrictionSaveCount >= 0 && this->topDevice() == this->rootDevice()) {
        if (!deviceRestriction.intersect(fClipRestrictionRect)) {
            deviceRestriction = SkIRect::MakeEmpty();
        }
    }
    AutoUpdateQRBounds aqr(this);
    this->topDevice()->replaceClip(deviceRestriction);
}

void SkCanvas::clipRRect(const SkRRect& rrect, SkClipOp op, bool doAA) {
    this->checkForDeferredSave();
    ClipEdgeStyle edgeStyle = doAA ? kSoft_ClipEdgeStyle : kHard_ClipEdgeStyle;
    if (rrect.isRect()) {
        this->onClipRect(rrect.getBounds(), op, edgeStyle);
    } else {
        this->onClipRRect(rrect, op, edgeStyle);
    }
}

void SkCanvas::onClipRRect(const SkRRect& rrect, SkClipOp op, ClipEdgeStyle edgeStyle) {
    bool isAA = kSoft_ClipEdgeStyle == edgeStyle;
    AutoUpdateQRBounds aqr(this);
    this->topDevice()->clipRRect(rrect, op, isAA);
}

void SkCanvas::clipPath(const SkPath& path, SkClipOp op, bool doAA) {
    this->checkForDeferredSave();
    ClipEdgeStyle edgeStyle = doAA ? kSoft_ClipEdgeStyle : kHard_ClipEdgeStyle;

    if (!path.isInverseFillType() && fMCRec->fMatrix.asM33().rectStaysRect()) {
        SkRect r;
        if (path.isRect(&r)) {
            this->onClipRect(r, op, edgeStyle);
            return;
        }
        SkRRect rrect;
        if (path.isOval(&r)) {
            rrect.setOval(r);
            this->onClipRRect(rrect, op, edgeStyle);
            return;
        }
        if (path.isRRect(&rrect)) {
            this->onClipRRect(rrect, op, edgeStyle);
            return;
        }
    }

    this->onClipPath(path, op, edgeStyle);
}

void SkCanvas::onClipPath(const SkPath& path, SkClipOp op, ClipEdgeStyle edgeStyle) {
    bool isAA = kSoft_ClipEdgeStyle == edgeStyle;
    AutoUpdateQRBounds aqr(this);
    this->topDevice()->clipPath(path, op, isAA);
}

void SkCanvas::clipShader(sk_sp<SkShader> sh, SkClipOp op) {
    if (sh) {
        if (sh->isOpaque()) {
            if (op == SkClipOp::kIntersect) {
                // no-op
            } else {
                SkASSERT(op == SkClipOp::kDifference);
                this->clipRect({0, 0, 0, 0});
            }
        } else {
            this->checkForDeferredSave();
            this->onClipShader(std::move(sh), op);
        }
    }
}

void SkCanvas::onClipShader(sk_sp<SkShader> sh, SkClipOp op) {
    AutoUpdateQRBounds aqr(this);
    this->topDevice()->clipShader(sh, op);
}

void SkCanvas::clipRegion(const SkRegion& rgn, SkClipOp op) {
    this->checkForDeferredSave();
    this->onClipRegion(rgn, op);
}

void SkCanvas::onClipRegion(const SkRegion& rgn, SkClipOp op) {
    AutoUpdateQRBounds aqr(this);
    this->topDevice()->clipRegion(rgn, op);
}

void SkCanvas::validateClip() const {
#ifdef SK_DEBUG
    SkRect tmp = this->computeDeviceClipBounds();
    if (this->isClipEmpty()) {
        SkASSERT(fQuickRejectBounds.isEmpty());
    } else {
        SkASSERT(tmp == fQuickRejectBounds);
    }
#endif
}

bool SkCanvas::androidFramework_isClipAA() const {
    return this->topDevice()->isClipAntiAliased();
}

void SkCanvas::temporary_internal_getRgnClip(SkRegion* rgn) {
    rgn->setEmpty();
    SkDevice* device = this->topDevice();
    if (device && device->isPixelAlignedToGlobal()) {
        device->android_utils_clipAsRgn(rgn);
        SkIPoint origin = device->getOrigin();
        if (origin.x() | origin.y()) {
            rgn->translate(origin.x(), origin.y());
        }
    }
}

bool SkCanvas::isClipEmpty() const { return this->topDevice()->isClipEmpty(); }
bool SkCanvas::isClipRect() const { return this->topDevice()->isClipRect(); }

bool SkCanvas::quickReject(const SkRect& src) const {
#ifdef SK_DEBUG
    this->validateClip();
#endif
    return skia_rs_canvas_quick_reject_rect(&fMCRec->fMatrix, src, fQuickRejectBounds);
}

bool SkCanvas::quickReject(const SkPath& path) const {
    return path.isEmpty() || this->quickReject(path.getBounds());
}

bool SkCanvas::nothingToDraw(const SkPaint& paint) const {
    return !this->topDevice()->surfaceProps().preservesTransparentDraws() && paint.nothingToDraw();
}

bool SkCanvas::internalQuickReject(const SkRect& bounds, const SkPaint& paint,
                                   const SkMatrix* matrix) {
    if (!bounds.isFinite() || this->nothingToDraw(paint)) {
        return true;
    }

    if (paint.canComputeFastBounds()) {
        SkRect tmp = matrix ? matrix->mapRect(bounds) : bounds;
        return this->quickReject(paint.computeFastBounds(tmp, &tmp));
    }

    return false;
}

SkRect SkCanvas::getLocalClipBounds() const {
    return skia_rs_canvas_compute_local_clip_bounds(
            this->getDeviceClipBounds(), &fMCRec->fMatrix);
}

SkIRect SkCanvas::getDeviceClipBounds() const {
    return this->computeDeviceClipBounds(/*outsetForAA=*/false).roundOut();
}

SkRect SkCanvas::computeDeviceClipBounds(bool outsetForAA) const {
    const SkDevice* dev = this->topDevice();
    return skia_rs_canvas_compute_device_clip_bounds(
            dev->isClipEmpty(), &dev->deviceToGlobal(), dev->devClipBounds(), outsetForAA);
}

SkMatrix SkCanvas::getTotalMatrix() const { return fMCRec->fMatrix.asM33(); }
SkM44 SkCanvas::getLocalToDevice() const { return fMCRec->fMatrix; }
GrRecordingContext* SkCanvas::recordingContext() const {
    return this->topDevice()->recordingContext();
}
skgpu::graphite::Recorder* SkCanvas::recorder() const { return this->topDevice()->recorder(); }
SkRecorder* SkCanvas::baseRecorder() const { return this->topDevice()->baseRecorder(); }

void SkCanvas::drawDRRect(const SkRRect& outer, const SkRRect& inner, const SkPaint& paint) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    int32_t action = skia_rs_canvas_draw_drrect_action(
            outer.isEmpty(), inner.isEmpty(), outer.getBounds(), inner.getBounds());
    if (action == 0) {
        return;
    }
    if (action == 1) {
        this->drawRRect(outer, paint);
        return;
    }
    this->onDrawDRRect(outer, inner, paint);
}

void SkCanvas::drawPaint(const SkPaint& paint) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    this->onDrawPaint(paint);
}

void SkCanvas::drawRect(const SkRect& r, const SkPaint& paint) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    this->onDrawRect(r.makeSorted(), paint);
}

void SkCanvas::drawClippedToSaveBehind(const SkPaint& paint) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    this->onDrawBehind(paint);
}

void SkCanvas::drawRegion(const SkRegion& region, const SkPaint& paint) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    if (region.isEmpty()) {
        return;
    }
    if (region.isRect()) {
        return this->drawIRect(region.getBounds(), paint);
    }
    this->onDrawRegion(region, paint);
}

void SkCanvas::drawOval(const SkRect& r, const SkPaint& paint) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    this->onDrawOval(r.makeSorted(), paint);
}

void SkCanvas::drawRRect(const SkRRect& rrect, const SkPaint& paint) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    this->onDrawRRect(rrect, paint);
}

void SkCanvas::drawPoints(PointMode mode, SkSpan<const SkPoint> pts, const SkPaint& paint) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    if (!pts.empty()) {
        this->onDrawPoints(mode, pts.size(), pts.data(), paint);
    }
}

void SkCanvas::drawVertices(const sk_sp<SkVertices>& vertices, SkBlendMode mode,
                            const SkPaint& paint) {
    this->drawVertices(vertices.get(), mode, paint);
}

void SkCanvas::drawVertices(const SkVertices* vertices, SkBlendMode mode, const SkPaint& paint) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    RETURN_ON_NULL(vertices);
    SkASSERT(vertices->priv().mode() != SkVertices::kTriangleFan_VertexMode);

#ifdef SK_BUILD_FOR_ANDROID_FRAMEWORK
    if (paint.getShader() && !vertices->priv().hasTexCoords()) {
        SkPaint noShaderPaint(paint);
        noShaderPaint.setShader(nullptr);
        this->onDrawVerticesObject(vertices, mode, noShaderPaint);
        return;
    }
#endif
    this->onDrawVerticesObject(vertices, mode, paint);
}

void SkCanvas::drawMesh(const SkMesh& mesh, sk_sp<SkBlender> blender, const SkPaint& paint) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    if (!blender) {
        blender = SkBlender::Mode(SkBlendMode::kModulate);
    }
    this->onDrawMesh(mesh, std::move(blender), paint);
}

void SkCanvas::drawPath(const SkPath& path, const SkPaint& paint) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    this->onDrawPath(path, paint);
}

static SkPaint clean_paint_for_lattice(const SkPaint* paint) {
    SkPaint cleaned;
    if (paint) {
        cleaned = *paint;
        cleaned.setMaskFilter(nullptr);
        cleaned.setAntiAlias(false);
    }
    return cleaned;
}

void SkCanvas::drawImageNine(const SkImage* image, const SkIRect& center, const SkRect& dst,
                             SkFilterMode filter, const SkPaint* paint) {
    RETURN_ON_NULL(image);

    const int xdivs[] = {center.fLeft, center.fRight};
    const int ydivs[] = {center.fTop, center.fBottom};

    Lattice lat;
    lat.fXDivs = xdivs;
    lat.fYDivs = ydivs;
    lat.fRectTypes = nullptr;
    lat.fXCount = lat.fYCount = 2;
    lat.fBounds = nullptr;
    lat.fColors = nullptr;
    this->drawImageLattice(image, lat, dst, filter, paint);
}

void SkCanvas::drawImageLattice(const SkImage* image, const Lattice& lattice, const SkRect& dst,
                                SkFilterMode filter, const SkPaint* paint) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    RETURN_ON_NULL(image);
    if (dst.isEmpty()) {
        return;
    }

    SkIRect bounds;
    Lattice latticePlusBounds = lattice;
    if (!latticePlusBounds.fBounds) {
        bounds = SkIRect::MakeWH(image->width(), image->height());
        latticePlusBounds.fBounds = &bounds;
    }

    SkPaint latticePaint = clean_paint_for_lattice(paint);
    if (SkLatticeIter::Valid(image->width(), image->height(), latticePlusBounds)) {
        this->onDrawImageLattice2(image, latticePlusBounds, dst, filter, &latticePaint);
    } else {
        this->drawImageRect(image, SkRect::MakeIWH(image->width(), image->height()), dst,
                            SkSamplingOptions(filter), &latticePaint, kStrict_SrcRectConstraint);
    }
}

void SkCanvas::drawAtlas(const SkImage* atlas, SkSpan<const SkRSXform> xform,
                         SkSpan<const SkRect> tex, SkSpan<const SkColor> colors, SkBlendMode mode,
                         const SkSamplingOptions& sampling, const SkRect* cull,
                         const SkPaint* paint) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    RETURN_ON_NULL(atlas);
    size_t count = std::min(xform.size(), tex.size());
    if (!colors.empty()) {
        count = std::min(count, colors.size());
    }
    if (count == 0) {
        return;
    }
    this->onDrawAtlas2(atlas, xform.data(), tex.data(), colors.data(), count, mode, sampling,
                       cull, paint);
}

void SkCanvas::drawAnnotation(const SkRect& rect, const char key[], SkData* value) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    if (key) {
        this->onDrawAnnotation(rect, key, value);
    }
}

void SkCanvas::private_draw_shadow_rec(const SkPath& path, const SkDrawShadowRec& rec) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    this->onDrawShadowRec(path, rec);
}

void SkCanvas::onDrawShadowRec(const SkPath& path, const SkDrawShadowRec& rec) {
    if (!this->predrawNotify()) {
        return;
    }
    this->topDevice()->drawShadow(this, path, rec);
}

void SkCanvas::experimental_DrawEdgeAAQuad(const SkRect& rect, const SkPoint clip[4],
                                           QuadAAFlags aaFlags, const SkColor4f& color,
                                           SkBlendMode mode) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    this->onDrawEdgeAAQuad(rect.makeSorted(), clip, aaFlags, color, mode);
}

void SkCanvas::experimental_DrawEdgeAAImageSet(const ImageSetEntry imageSet[], int cnt,
                                               const SkPoint dstClips[],
                                               const SkMatrix preViewMatrices[],
                                               const SkSamplingOptions& sampling,
                                               const SkPaint* paint,
                                               SrcRectConstraint constraint) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    if (paint && (paint->getImageFilter() || paint->getMaskFilter()) && cnt == 1) {
        const auto& entry = imageSet[0];
        const SkMatrix* pm =
                entry.fMatrixIndex >= 0 ? &preViewMatrices[entry.fMatrixIndex] : nullptr;
        if (skia_rs_canvas_edge_aa_single_can_map_dst(entry.fHasClip, entry.fMatrixIndex, pm)) {
            SkRect dst = entry.fDstRect;
            if (pm) {
                pm->mapRect(&dst);
            }
            this->drawImageRect(entry.fImage.get(), entry.fSrcRect, dst,
                                sampling, paint, constraint);
            return;
        }
    }
    this->onDrawEdgeAAImageSet2(imageSet, cnt, dstClips, preViewMatrices, sampling, paint,
                                constraint);
}

void SkCanvas::onDiscard() {
    if (fSurfaceBase) {
        sk_ignore_unused_variable(fSurfaceBase->aboutToDraw(SkSurface::kDiscard_ContentChangeMode));
    }
}

void SkCanvas::onDrawPaint(const SkPaint& paint) { this->internalDrawPaint(paint); }

void SkCanvas::internalDrawPaint(const SkPaint& paint) {
    if (this->nothingToDraw(paint) || this->isClipEmpty()) {
        return;
    }
    auto layer = this->aboutToDraw(paint, nullptr, PredrawFlags::kCheckForOverwrite);
    if (layer) {
        this->topDevice()->drawPaint(layer->paint());
    }
}

void SkCanvas::onDrawPoints(PointMode mode, size_t count, const SkPoint pts[],
                            const SkPaint& paint) {
    if ((long)count <= 0 || this->nothingToDraw(paint)) {
        return;
    }
    SkASSERT(pts != nullptr);

    SkPaint strokePaint = paint;
    strokePaint.setStyle(SkPaint::kStroke_Style);

    SkRect boundsStorage;
    const SkRect* boundsPtr = nullptr;

    if (paint.getImageFilter() || paint.getMaskFilter()) {
        auto bounds = SkRect::Bounds({pts, count});
        if (!bounds) {
            return;
        }
        if (this->internalQuickReject(bounds.value(), strokePaint)) {
            return;
        }
        boundsStorage = bounds.value();
        boundsPtr = &boundsStorage;
    }

    auto layer = this->aboutToDraw(strokePaint, boundsPtr);
    if (layer) {
        this->topDevice()->drawPoints(mode, {pts, count}, layer->paint());
    }
}

const SkBlurMaskFilterImpl* SkCanvas::canAttemptBlurredRRectDraw(const SkPaint& paint) const {
    if (!this->topDevice()->useDrawCoverageMaskForMaskFilters()) {
        return nullptr;
    }
    if (paint.getPathEffect()) {
        return nullptr;
    }
    if (SkStrokeRec(paint).getStyle() != SkStrokeRec::kFill_Style) {
        return nullptr;
    }
    const SkMaskFilterBase* maskFilter = as_MFB(paint.getMaskFilter());
    if (!maskFilter || maskFilter->type() != SkMaskFilterBase::Type::kBlur) {
        return nullptr;
    }
    const SkBlurMaskFilterImpl* blurMaskFilter =
            static_cast<const SkBlurMaskFilterImpl*>(maskFilter);
    if (blurMaskFilter->blurStyle() != kNormal_SkBlurStyle) {
        return nullptr;
    }
    return blurMaskFilter;
}

std::optional<AutoLayerForImageFilter> SkCanvas::attemptBlurredRRectDraw(
        const SkRRect& rrect,
        const SkBlurMaskFilterImpl* blurMaskFilter,
        const SkPaint& paint,
        SkEnumBitMask<PredrawFlags> flags) {
    SkASSERT(blurMaskFilter && blurMaskFilter == this->canAttemptBlurredRRectDraw(paint) &&
             !(flags & PredrawFlags::kSkipMaskFilterAutoLayer));
    const SkRect& bounds = rrect.getBounds();

    auto layer = this->aboutToDraw(paint, &bounds, flags | PredrawFlags::kSkipMaskFilterAutoLayer);
    if (!layer) {
        return std::nullopt;
    }

    const float deviceSigma = blurMaskFilter->computeXformedDeviceSigma(this->getTotalMatrix());
    const SkV2 localSigma = blurMaskFilter->computeXformedLocalSigma(this->getTotalMatrix());
    if (this->topDevice()->drawBlurredRRect(rrect, layer->paint(), localSigma, deviceSigma)) {
        return std::nullopt;
    }

    layer->addMaskFilterLayer(&bounds);
    return layer;
}

void SkCanvas::onDrawRect(const SkRect& r, const SkPaint& paint) {
    SkASSERT(r.isSorted());
    if (this->internalQuickReject(r, paint)) {
        return;
    }

    std::optional<AutoLayerForImageFilter> layer;
    constexpr PredrawFlags kPredrawFlags = PredrawFlags::kCheckForOverwrite;

    if (const SkBlurMaskFilterImpl* blurMaskFilter = this->canAttemptBlurredRRectDraw(paint)) {
        layer = this->attemptBlurredRRectDraw(
                SkRRect::MakeRect(r), blurMaskFilter, paint, kPredrawFlags);
    } else {
        layer = this->aboutToDraw(paint, &r, kPredrawFlags);
    }

    if (layer) {
        this->topDevice()->drawRect(r, layer->paint());
    }
}

void SkCanvas::onDrawRegion(const SkRegion& region, const SkPaint& paint) {
    const SkRect bounds = SkRect::Make(region.getBounds());
    if (this->internalQuickReject(bounds, paint)) {
        return;
    }

    auto layer = this->aboutToDraw(paint, &bounds);
    if (layer) {
        this->topDevice()->drawRegion(region, layer->paint());
    }
}

void SkCanvas::onDrawBehind(const SkPaint& paint) {
    SkDevice* dev = this->topDevice();
    if (!dev) {
        return;
    }

    SkIRect bounds;
    SkDeque::Iter iter(fMCStack, SkDeque::Iter::kBack_IterStart);
    for (;;) {
        const MCRec* rec = (const MCRec*)iter.prev();
        if (!rec) {
            return;
        }
        if (rec->fBackImage) {
            SkASSERT(dev == rec->fDevice);
            bounds = SkIRect::MakeXYWH(rec->fBackImage->fLoc.fX, rec->fBackImage->fLoc.fY,
                                       rec->fBackImage->fImage->width(),
                                       rec->fBackImage->fImage->height());
            break;
        }
    }

    dev->pushClipStack();
    {
        SkAutoDeviceTransformRestore adtr(dev, SkM44());
        dev->clipRect(SkRect::Make(bounds), SkClipOp::kIntersect, /*aa=*/false);
    }

    auto layer = this->aboutToDraw(paint);
    if (layer) {
        this->topDevice()->drawPaint(layer->paint());
    }

    dev->popClipStack();
}

void SkCanvas::onDrawOval(const SkRect& oval, const SkPaint& paint) {
    SkASSERT(oval.isSorted());
    if (this->internalQuickReject(oval, paint)) {
        return;
    }

    std::optional<AutoLayerForImageFilter> layer;
    if (const SkBlurMaskFilterImpl* blurMaskFilter = this->canAttemptBlurredRRectDraw(paint)) {
        layer = this->attemptBlurredRRectDraw(
                SkRRect::MakeOval(oval), blurMaskFilter, paint, PredrawFlags::kNone);
    } else {
        layer = this->aboutToDraw(paint, &oval);
    }

    if (layer) {
        this->topDevice()->drawOval(oval, layer->paint());
    }
}

void SkCanvas::onDrawArc(const SkRect& oval, SkScalar startAngle,
                         SkScalar sweepAngle, bool useCenter, const SkPaint& paint) {
    SkASSERT(oval.isSorted());
    if (this->internalQuickReject(oval, paint)) {
        return;
    }

    std::optional<AutoLayerForImageFilter> layer;
    if (const SkBlurMaskFilterImpl* blurMaskFilter = this->canAttemptBlurredRRectDraw(paint);
        blurMaskFilter && SkScalarAbs(sweepAngle) >= 360.f) {
        layer = this->attemptBlurredRRectDraw(
                SkRRect::MakeOval(oval), blurMaskFilter, paint, PredrawFlags::kNone);
    } else {
        layer = this->aboutToDraw(paint, &oval);
    }

    if (layer) {
        this->topDevice()->drawArc(SkArc::Make(oval, startAngle, sweepAngle, useCenter),
                                   layer->paint());
    }
}

void SkCanvas::onDrawRRect(const SkRRect& rrect, const SkPaint& paint) {
    const SkRect& bounds = rrect.getBounds();

    if (rrect.isRect()) {
        this->SkCanvas::drawRect(bounds, paint);
        return;
    } else if (rrect.isOval()) {
        this->SkCanvas::drawOval(bounds, paint);
        return;
    }

    if (this->internalQuickReject(bounds, paint)) {
        return;
    }

    std::optional<AutoLayerForImageFilter> layer;
    if (const SkBlurMaskFilterImpl* blurMaskFilter = this->canAttemptBlurredRRectDraw(paint)) {
        layer = this->attemptBlurredRRectDraw(rrect, blurMaskFilter, paint, PredrawFlags::kNone);
    } else {
        layer = this->aboutToDraw(paint, &bounds);
    }

    if (layer) {
        this->topDevice()->drawRRect(rrect, layer->paint());
    }
}

void SkCanvas::onDrawDRRect(const SkRRect& outer, const SkRRect& inner, const SkPaint& paint) {
    const SkRect& bounds = outer.getBounds();
    if (this->internalQuickReject(bounds, paint)) {
        return;
    }

    auto layer = this->aboutToDraw(paint, &bounds);
    if (layer) {
        this->topDevice()->drawDRRect(outer, inner, layer->paint());
    }
}

void SkCanvas::onDrawPath(const SkPath& path, const SkPaint& paint) {
    if (!path.isFinite()) {
        return;
    }

    const SkRect& pathBounds = path.getBounds();
    if (!path.isInverseFillType() && this->internalQuickReject(pathBounds, paint)) {
        return;
    }
    if (path.isInverseFillType() && pathBounds.width() <= 0 && pathBounds.height() <= 0) {
        this->internalDrawPaint(paint);
        return;
    }

    auto layer = this->aboutToDraw(paint, path.isInverseFillType() ? nullptr : &pathBounds);
    if (layer) {
        this->topDevice()->drawPath(path, layer->paint());
    }
}

static SkPaint clean_paint_for_drawImage(const SkPaint* paint) {
    SkPaint cleaned;
    if (paint) {
        cleaned = *paint;
        cleaned.setStyle(SkPaint::kFill_Style);
        cleaned.setPathEffect(nullptr);
    }
    return cleaned;
}

static SkPaint clean_paint_for_drawVertices(SkPaint paint) {
    paint.setStyle(SkPaint::kFill_Style);
    paint.setMaskFilter(nullptr);
    paint.setPathEffect(nullptr);
    return paint;
}

void SkCanvas::onDrawImage2(const SkImage*, SkScalar, SkScalar,
                            const SkSamplingOptions&, const SkPaint*) {
    SkUNREACHABLE;
}

static SkSamplingOptions clean_sampling_for_constraint(
        const SkSamplingOptions& sampling,
        SkCanvas::SrcRectConstraint constraint) {
    SkiaRsCleanedSamplingFlags cf = skia_rs_canvas_clean_sampling_for_constraint(
            constraint == SkCanvas::kStrict_SrcRectConstraint,
            sampling.mipmap != SkMipmapMode::kNone,
            sampling.isAniso());
    if (cf.changed) {
        if (cf.mode == 1) {
            return SkSamplingOptions(sampling.filter);
        }
        if (cf.mode == 2) {
            return SkSamplingOptions(SkFilterMode::kLinear);
        }
    }
    return sampling;
}

void SkCanvas::onDrawImageRect2(const SkImage* image, const SkRect& src, const SkRect& dst,
                                const SkSamplingOptions& sampling, const SkPaint* paint,
                                SrcRectConstraint constraint) {
    SkPaint realPaint = clean_paint_for_drawImage(paint);
    SkSamplingOptions realSampling = clean_sampling_for_constraint(sampling, constraint);

    if (this->internalQuickReject(dst, realPaint)) {
        return;
    }

    if (this->topDevice()->shouldDrawAsTiledImageRect()) {
        if (this->topDevice()->drawAsTiledImageRect(
                    this, image, &src, dst, realSampling, realPaint, constraint)) {
            return;
        }
    }

    if (realPaint.getImageFilter() && !image->isAlphaOnly() && !realPaint.getMaskFilter()) {
        SkDevice* device = this->topDevice();

        skif::ParameterSpace<SkRect> imageBounds{dst};
        skif::DeviceSpace<SkIRect> outputBounds{device->devClipBounds()};
        FilterToSpan filterAsSpan(realPaint.getImageFilter());
        auto mappingAndBounds = get_layer_mapping_and_bounds(filterAsSpan,
                                                             device->localToDevice44(),
                                                             outputBounds,
                                                             imageBounds);
        if (!mappingAndBounds) {
            return;
        }
        if (!this->predrawNotify()) {
            return;
        }

        auto backend = device->createImageFilteringBackend(
                device->surfaceProps(),
                image_filter_color_type(device->imageInfo().colorInfo()));
        auto [mapping, srcBounds] = *mappingAndBounds;
        skif::Stats stats;
        skif::Context ctx{std::move(backend),
                          mapping,
                          srcBounds,
                          skif::FilterResult{},
                          device->imageInfo().colorSpace(),
                          &stats};

        auto source = skif::FilterResult::MakeFromImage(
                ctx, sk_ref_sp(image), src, imageBounds, sampling);
        source = apply_alpha_and_colorfilter(ctx, source, realPaint);

        ctx = ctx.withNewDesiredOutput(mapping.deviceToLayer(outputBounds))
                 .withNewSource(source);
        auto result = as_IFB(realPaint.getImageFilter())->filterImage(ctx);
        result.draw(ctx, device, realPaint.getBlender());
        stats.reportStats();
        return;
    }

    if (realPaint.getMaskFilter() && this->topDevice()->useDrawCoverageMaskForMaskFilters()) {
        auto [drawDstRect, shader] = SkImageShader::MakeForDrawRect(
                image, realPaint, sampling, src, dst, constraint == kStrict_SrcRectConstraint);
        if (drawDstRect.isEmpty() || !shader) {
            return;
        }
        realPaint.setShader(std::move(shader));
        this->drawRect(drawDstRect, realPaint);
        return;
    }

    auto layer = this->aboutToDraw(
            realPaint, &dst,
            PredrawFlags::kCheckForOverwrite |
                    (image->isOpaque() ? PredrawFlags::kOpaqueShaderOverride
                                       : PredrawFlags::kNonOpaqueShaderOverride));
    if (layer) {
        this->topDevice()->drawImageRect(image, &src, dst, realSampling, layer->paint(),
                                         constraint);
    }
}

void SkCanvas::onDrawImageLattice2(const SkImage* image, const Lattice& lattice, const SkRect& dst,
                                   SkFilterMode filter, const SkPaint* paint) {
    SkPaint realPaint = clean_paint_for_drawImage(paint);

    if (this->internalQuickReject(dst, realPaint)) {
        return;
    }

    auto layer = this->aboutToDraw(realPaint, &dst);
    if (layer) {
        this->topDevice()->drawImageLattice(image, lattice, dst, filter, layer->paint());
    }
}

void SkCanvas::drawImage(const SkImage* image, SkScalar x, SkScalar y,
                         const SkSamplingOptions& sampling, const SkPaint* paint) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    RETURN_ON_NULL(image);

    this->drawImageRect(image,
                        SkRect::MakeWH(image->width(), image->height()),
                        SkRect::MakeXYWH(x, y, image->width(), image->height()),
                        sampling,
                        paint,
                        kFast_SrcRectConstraint);
}

void SkCanvas::drawImageRect(const SkImage* image, const SkRect& src, const SkRect& dst,
                             const SkSamplingOptions& sampling, const SkPaint* paint,
                             SrcRectConstraint constraint) {
    RETURN_ON_NULL(image);
    if (!skia_rs_canvas_fillable_rect(dst) || !skia_rs_canvas_fillable_rect(src)) {
        return;
    }
    this->onDrawImageRect2(image, src, dst, sampling, paint, constraint);
}

void SkCanvas::drawImageRect(const SkImage* image, const SkRect& dst,
                             const SkSamplingOptions& sampling, const SkPaint* paint) {
    RETURN_ON_NULL(image);
    this->drawImageRect(image, SkRect::MakeIWH(image->width(), image->height()), dst, sampling,
                        paint, kFast_SrcRectConstraint);
}

sktext::GlyphRunBuilder* SkCanvas::obtainGlyphRunBuilder() {
    if (fRunBuildersUsed >= fRunBuilders.size()) {
        fRunBuilders.push_back(std::make_unique<sktext::GlyphRunBuilder>());
    }
    sktext::GlyphRunBuilder* result = fRunBuilders[fRunBuildersUsed].get();
    fRunBuildersUsed++;
    return result;
}

void SkCanvas::releaseGlyphRunBuilder() {
    SkASSERT(fRunBuildersUsed > 0);
    fRunBuildersUsed--;
}

class AutoGlyphRunBuilder {
public:
    SK_ONLY_STACK_ALLOCATABLE;

    AutoGlyphRunBuilder(SkCanvas* canvas)
            : fCanvas(canvas), fGlyphRunBuilder(canvas->obtainGlyphRunBuilder()) {}
    ~AutoGlyphRunBuilder() { fCanvas->releaseGlyphRunBuilder(); }

    sktext::GlyphRunBuilder* operator->() { return fGlyphRunBuilder; }

private:
    SkCanvas* fCanvas;
    sktext::GlyphRunBuilder* fGlyphRunBuilder;
};

void SkCanvas::onDrawTextBlob(const SkTextBlob* blob, SkScalar x, SkScalar y,
                              const SkPaint& paint) {
    AutoGlyphRunBuilder scratchBuilder(this);
    auto glyphRunList = scratchBuilder->blobToGlyphRunList(*blob, {x, y});
    this->onDrawGlyphRunList(glyphRunList, paint);
}

void SkCanvas::onDrawGlyphRunList(const sktext::GlyphRunList& glyphRunList, const SkPaint& paint) {
    SkRect bounds = glyphRunList.sourceBoundsWithOrigin();
    if (this->internalQuickReject(bounds, paint)) {
        return;
    }

    auto layer = this->aboutToDraw(paint, &bounds, PredrawFlags::kSkipMaskFilterAutoLayer);
    if (layer) {
        this->topDevice()->drawGlyphRunList(this, glyphRunList, layer->paint());
    }
}

sk_sp<Slug> SkCanvas::convertBlobToSlug(
        const SkTextBlob& blob, SkPoint origin, const SkPaint& paint) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    AutoGlyphRunBuilder scratchBuilder(this);
    auto glyphRunList = scratchBuilder->blobToGlyphRunList(blob, origin);
    return this->onConvertGlyphRunListToSlug(glyphRunList, paint);
}

sk_sp<Slug> SkCanvas::onConvertGlyphRunListToSlug(const sktext::GlyphRunList& glyphRunList,
                                                  const SkPaint& paint) {
    SkRect bounds = glyphRunList.sourceBoundsWithOrigin();
    if (bounds.isEmpty() || !bounds.isFinite() || this->nothingToDraw(paint)) {
        return nullptr;
    }
    auto layer = this->aboutToDraw(paint, &bounds, PredrawFlags::kSkipMaskFilterAutoLayer);
    if (layer) {
        return this->topDevice()->convertGlyphRunListToSlug(glyphRunList, layer->paint());
    }
    return nullptr;
}

void SkCanvas::drawSlug(const Slug* slug, const SkPaint& paint) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    if (slug) {
        this->onDrawSlug(slug, paint);
    }
}

void SkCanvas::onDrawSlug(const Slug* slug, const SkPaint& paint) {
    SkRect bounds = slug->sourceBoundsWithOrigin();
    if (this->internalQuickReject(bounds, paint)) {
        return;
    }
    auto layer = this->aboutToDraw(paint, &bounds, PredrawFlags::kSkipMaskFilterAutoLayer);
    if (layer) {
        this->topDevice()->drawSlug(this, slug, layer->paint());
    }
}

void SkCanvas::drawSimpleText(const void* text, size_t byteLength, SkTextEncoding encoding,
                              SkScalar x, SkScalar y, const SkFont& font, const SkPaint& paint) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    if (byteLength) {
        sk_msan_assert_initialized(text, SkTAddOffset<const void>(text, byteLength));
        AutoGlyphRunBuilder scratchBuilder(this);
        const sktext::GlyphRunList& glyphRunList =
                scratchBuilder->textToGlyphRunList(font, paint, text, byteLength, {x, y}, encoding);
        if (!glyphRunList.empty()) {
            this->onDrawGlyphRunList(glyphRunList, paint);
        }
    }
}

void SkCanvas::drawGlyphs(SkSpan<const SkGlyphID> glyphs, SkSpan<const SkPoint> positions,
                          SkSpan<const uint32_t> clusters, SkSpan<const char> utf8text,
                          SkPoint origin, const SkFont& font, const SkPaint& paint) {
    if (glyphs.empty()) {
        return;
    }
    AutoGlyphRunBuilder scratchBuilder(this);

    sktext::GlyphRun glyphRun{
            font, positions, glyphs, utf8text, clusters, SkSpan<SkVector>()};
    sktext::GlyphRunList glyphRunList = scratchBuilder->makeGlyphRunList(glyphRun, paint, origin);
    this->onDrawGlyphRunList(glyphRunList, paint);
}

void SkCanvas::drawGlyphs(SkSpan<const SkGlyphID> glyphs, SkSpan<const SkPoint> positions,
                          SkPoint origin, const SkFont& font, const SkPaint& paint) {
    if (glyphs.empty()) {
        return;
    }
    AutoGlyphRunBuilder scratchBuilder(this);

    sktext::GlyphRun glyphRun{
            font, positions, glyphs, SkSpan<const char>(), SkSpan<const uint32_t>(),
            SkSpan<SkVector>()};
    sktext::GlyphRunList glyphRunList = scratchBuilder->makeGlyphRunList(glyphRun, paint, origin);
    this->onDrawGlyphRunList(glyphRunList, paint);
}

void SkCanvas::drawGlyphsRSXform(SkSpan<const SkGlyphID> glyphs, SkSpan<const SkRSXform> xforms,
                                 SkPoint origin, const SkFont& font, const SkPaint& paint) {
    if (glyphs.empty()) {
        return;
    }
    AutoGlyphRunBuilder scratchBuilder(this);
    auto [positions, rotateScales] = scratchBuilder->convertRSXForm(xforms);

    sktext::GlyphRun glyphRun{
            font, positions, glyphs, SkSpan<const char>(), SkSpan<const uint32_t>(), rotateScales};
    sktext::GlyphRunList glyphRunList = scratchBuilder->makeGlyphRunList(glyphRun, paint, origin);
    this->onDrawGlyphRunList(glyphRunList, paint);
}

void SkCanvas::drawTextBlob(const SkTextBlob* blob, SkScalar x, SkScalar y,
                            const SkPaint& paint) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    RETURN_ON_NULL(blob);
    RETURN_ON_FALSE(blob->bounds().makeOffset(x, y).isFinite());

    int32_t totalGlyphCount = 0;
    SkTextBlob::Iter i(*blob);
    SkTextBlob::Iter::Run r;
    while (i.next(&r)) {
        RETURN_ON_FALSE(skia_rs_canvas_text_blob_add_glyph_count(
                totalGlyphCount, r.fGlyphCount, &totalGlyphCount));
    }

    this->onDrawTextBlob(blob, x, y, paint);
}

void SkCanvas::onDrawVerticesObject(const SkVertices* vertices, SkBlendMode bmode,
                                    const SkPaint& paint) {
    SkPaint simplePaint = clean_paint_for_drawVertices(paint);

    const SkRect& bounds = vertices->bounds();
    if (this->internalQuickReject(bounds, simplePaint)) {
        return;
    }

    auto layer = this->aboutToDraw(simplePaint, &bounds);
    if (layer) {
        this->topDevice()->drawVertices(vertices, SkBlender::Mode(bmode), layer->paint());
    }
}

void SkCanvas::onDrawMesh(const SkMesh& mesh, sk_sp<SkBlender> blender, const SkPaint& paint) {
    SkPaint simplePaint = clean_paint_for_drawVertices(paint);
    auto layer = this->aboutToDraw(simplePaint, nullptr);
    if (layer) {
        this->topDevice()->drawMesh(mesh, std::move(blender), paint);
    }
}

void SkCanvas::drawPatch(const SkPoint cubics[12], const SkColor colors[4],
                         const SkPoint texCoords[4], SkBlendMode bmode,
                         const SkPaint& paint) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    if (nullptr == cubics) {
        return;
    }
    this->onDrawPatch(cubics, colors, texCoords, bmode, paint);
}

void SkCanvas::onDrawPatch(const SkPoint cubics[12], const SkColor colors[4],
                           const SkPoint texCoords[4], SkBlendMode bmode,
                           const SkPaint& paint) {
    auto bounds = SkRect::Bounds({cubics, (size_t)SkPatchUtils::kNumCtrlPts});
    if (!bounds) {
        return;
    }

    SkPaint simplePaint = clean_paint_for_drawVertices(paint);
    if (this->internalQuickReject(bounds.value(), simplePaint)) {
        return;
    }

    auto r = bounds.value();
    auto layer = this->aboutToDraw(simplePaint, &r);
    if (layer) {
        this->topDevice()->drawPatch(cubics, colors, texCoords, SkBlender::Mode(bmode),
                                     layer->paint());
    }
}

void SkCanvas::drawDrawable(SkDrawable* dr, SkScalar x, SkScalar y) {
#ifndef SK_BUILD_FOR_ANDROID_FRAMEWORK
    TRACE_EVENT0("skia", TRACE_FUNC);
#endif
    RETURN_ON_NULL(dr);
    if (x || y) {
        SkMatrix matrix = SkMatrix::Translate(x, y);
        this->onDrawDrawable(dr, &matrix);
    } else {
        this->onDrawDrawable(dr, nullptr);
    }
}

void SkCanvas::drawDrawable(SkDrawable* dr, const SkMatrix* matrix) {
#ifndef SK_BUILD_FOR_ANDROID_FRAMEWORK
    TRACE_EVENT0("skia", TRACE_FUNC);
#endif
    RETURN_ON_NULL(dr);
    if (matrix && matrix->isIdentity()) {
        matrix = nullptr;
    }
    this->onDrawDrawable(dr, matrix);
}

void SkCanvas::onDrawDrawable(SkDrawable* dr, const SkMatrix* matrix) {
    if (this->predrawNotify()) {
        this->topDevice()->drawDrawable(this, dr, matrix);
    }
}

void SkCanvas::onDrawAtlas2(const SkImage* atlas, const SkRSXform xform[], const SkRect tex[],
                            const SkColor colors[], int count, SkBlendMode bmode,
                            const SkSamplingOptions& sampling, const SkRect* cull,
                            const SkPaint* paint) {
    SkPaint realPaint = clean_paint_for_drawVertices(clean_paint_for_drawImage(paint));
    realPaint.setShader(atlas->makeShader(sampling));

    if (cull && this->internalQuickReject(*cull, realPaint)) {
        return;
    }

    SkASSERT(!realPaint.getMaskFilter());
    auto layer = this->aboutToDraw(realPaint);
    if (layer) {
        size_t N = SkToSizeT(count);
        this->topDevice()->drawAtlas({xform, N}, {tex, N}, {colors, colors ? N : 0},
                                     SkBlender::Mode(bmode), layer->paint());
    }
}

void SkCanvas::onDrawAnnotation(const SkRect& rect, const char key[], SkData* value) {
    SkASSERT(key);
    if (this->predrawNotify()) {
        this->topDevice()->drawAnnotation(rect, key, value);
    }
}

void SkCanvas::onDrawEdgeAAQuad(const SkRect& r, const SkPoint clip[4], QuadAAFlags edgeAA,
                                const SkColor4f& color, SkBlendMode mode) {
    SkASSERT(r.isSorted());

    SkPaint paint{color};
    paint.setBlendMode(mode);
    if (this->internalQuickReject(r, paint)) {
        return;
    }

    if (this->predrawNotify()) {
        this->topDevice()->drawEdgeAAQuad(r, clip, edgeAA, color, mode);
    }
}

void SkCanvas::onDrawEdgeAAImageSet2(const ImageSetEntry imageSet[], int count,
                                     const SkPoint dstClips[], const SkMatrix preViewMatrices[],
                                     const SkSamplingOptions& sampling, const SkPaint* paint,
                                     SrcRectConstraint constraint) {
    if (count <= 0) {
        return;
    }

    SkPaint realPaint = clean_paint_for_drawImage(paint);
    SkSamplingOptions realSampling = clean_sampling_for_constraint(sampling, constraint);

    bool needsAutoLayer = SkToBool(realPaint.getImageFilter() || realPaint.getMaskFilter());
    bool setBoundsValid = count == 1 || needsAutoLayer;
    SkRect setBounds = imageSet[0].fDstRect;
    if (imageSet[0].fMatrixIndex >= 0) {
        preViewMatrices[imageSet[0].fMatrixIndex].mapRect(&setBounds);
    }
    if (needsAutoLayer) {
        for (int i = 1; i < count; ++i) {
            SkRect entryBounds = imageSet[i].fDstRect;
            if (imageSet[i].fMatrixIndex >= 0) {
                preViewMatrices[imageSet[i].fMatrixIndex].mapRect(&entryBounds);
            }
            setBounds.joinPossiblyEmptyRect(entryBounds);
        }
    }

    if (setBoundsValid && this->internalQuickReject(setBounds, realPaint)) {
        return;
    }

    if (realPaint.getMaskFilter() && this->topDevice()->useDrawCoverageMaskForMaskFilters()) {
        int dstClipIndex = 0;
        for (int i = 0; i < count; ++i) {
            SkPaint imagePaint = realPaint;
            auto [drawDstRect, shader] =
                    SkImageShader::MakeForDrawRect(imageSet[i].fImage.get(),
                                                   imagePaint,
                                                   sampling,
                                                   imageSet[i].fSrcRect,
                                                   imageSet[i].fDstRect,
                                                   constraint == kStrict_SrcRectConstraint);
            if (drawDstRect.isEmpty() || !shader) {
                return;
            }
            imagePaint.setShader(std::move(shader));
            auto layer = this->aboutToDraw(imagePaint, &drawDstRect);
            if (layer) {
                if (imageSet[i].fMatrixIndex >= 0) {
                    this->topDevice()->setLocalToDevice(
                            this->topDevice()->localToDevice44() *
                            SkM44(preViewMatrices[imageSet[i].fMatrixIndex]));
                }
                this->topDevice()->drawEdgeAAQuad(
                        drawDstRect,
                        imageSet[i].fHasClip ? dstClips + dstClipIndex : nullptr,
                        (QuadAAFlags)imageSet[i].fAAFlags,
                        layer->paint().getColor4f(),
                        SkBlendMode::kSrcOver);
            }
            dstClipIndex += 4 * imageSet[i].fHasClip;
        }
        return;
    }

    auto layer = this->aboutToDraw(realPaint, setBoundsValid ? &setBounds : nullptr);
    if (layer) {
        this->topDevice()->drawEdgeAAImageSet(imageSet, count, dstClips, preViewMatrices,
                                              realSampling, layer->paint(), constraint);
    }
}

void SkCanvas::drawColor(const SkColor4f& c, SkBlendMode mode) {
    SkPaint paint;
    paint.setColor(c);
    paint.setBlendMode(mode);
    this->drawPaint(paint);
}

void SkCanvas::drawPoint(SkScalar x, SkScalar y, const SkPaint& paint) {
    const SkPoint pt[1] = {{x, y}};
    this->drawPoints(kPoints_PointMode, pt, paint);
}

void SkCanvas::drawLine(SkScalar x0, SkScalar y0, SkScalar x1, SkScalar y1, const SkPaint& paint) {
    SkPoint pts[2];
    pts[0].set(x0, y0);
    pts[1].set(x1, y1);
    this->drawPoints(kLines_PointMode, pts, paint);
}

void SkCanvas::drawCircle(SkScalar cx, SkScalar cy, SkScalar radius, const SkPaint& paint) {
    SkRect r = skia_rs_canvas_draw_circle_bounds(cx, cy, radius);
    this->drawOval(r, paint);
}

void SkCanvas::drawRoundRect(const SkRect& r, SkScalar rx, SkScalar ry, const SkPaint& paint) {
    if (skia_rs_canvas_draw_round_rect_is_rrect(rx, ry)) {
        SkRRect rrect;
        rrect.setRectXY(r, rx, ry);
        this->drawRRect(rrect, paint);
    } else {
        this->drawRect(r, paint);
    }
}

void SkCanvas::drawArc(const SkRect& oval, SkScalar startAngle, SkScalar sweepAngle,
                       bool useCenter, const SkPaint& paint) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    if (skia_rs_canvas_draw_arc_should_skip(oval, sweepAngle)) {
        return;
    }
    this->onDrawArc(oval, startAngle, sweepAngle, useCenter, paint);
}

#ifdef SK_DISABLE_SKPICTURE
void SkCanvas::drawPicture(const SkPicture*, const SkMatrix*, const SkPaint*) {}
void SkCanvas::onDrawPicture(const SkPicture*, const SkMatrix*, const SkPaint*) {}
#else
void SkCanvas::drawPicture(const SkPicture* picture, const SkMatrix* matrix, const SkPaint* paint) {
    TRACE_EVENT0("skia", TRACE_FUNC);
    RETURN_ON_NULL(picture);

    if (matrix && matrix->isIdentity()) {
        matrix = nullptr;
    }
    if (picture->approximateOpCount() <= kMaxPictureOpsToUnrollInsteadOfRef) {
        SkAutoCanvasMatrixPaint acmp(this, matrix, paint, picture->cullRect());
        picture->playback(this);
    } else {
        this->onDrawPicture(picture, matrix, paint);
    }
}

void SkCanvas::onDrawPicture(const SkPicture* picture, const SkMatrix* matrix,
                             const SkPaint* paint) {
    if (this->internalQuickReject(picture->cullRect(), paint ? *paint : SkPaint{}, matrix)) {
        return;
    }

    SkAutoCanvasMatrixPaint acmp(this, matrix, paint, picture->cullRect());
    picture->playback(this);
}
#endif

SkCanvas::ImageSetEntry::ImageSetEntry() = default;
SkCanvas::ImageSetEntry::~ImageSetEntry() = default;
SkCanvas::ImageSetEntry::ImageSetEntry(const ImageSetEntry&) = default;
SkCanvas::ImageSetEntry& SkCanvas::ImageSetEntry::operator=(const ImageSetEntry&) = default;

SkCanvas::ImageSetEntry::ImageSetEntry(sk_sp<const SkImage> image, const SkRect& srcRect,
                                       const SkRect& dstRect, int matrixIndex, float alpha,
                                       unsigned aaFlags, bool hasClip)
        : fImage(std::move(image))
        , fSrcRect(srcRect)
        , fDstRect(dstRect)
        , fMatrixIndex(matrixIndex)
        , fAlpha(alpha)
        , fAAFlags(aaFlags)
        , fHasClip(hasClip) {}

SkCanvas::ImageSetEntry::ImageSetEntry(sk_sp<const SkImage> image, const SkRect& srcRect,
                                       const SkRect& dstRect, float alpha, unsigned aaFlags)
        : fImage(std::move(image))
        , fSrcRect(srcRect)
        , fDstRect(dstRect)
        , fAlpha(alpha)
        , fAAFlags(aaFlags) {}

std::unique_ptr<SkCanvas> SkCanvas::MakeRasterDirect(const SkImageInfo& info, void* pixels,
                                                     size_t rowBytes, const SkSurfaceProps* props) {
    if (!SkSurfaceValidateRasterInfo(info, rowBytes)) {
        return nullptr;
    }

    SkBitmap bitmap;
    if (!bitmap.installPixels(info, pixels, rowBytes)) {
        return nullptr;
    }

    return props ? std::make_unique<SkCanvas>(bitmap, *props)
                 : std::make_unique<SkCanvas>(bitmap);
}

SkNoDrawCanvas::SkNoDrawCanvas(int width, int height)
        : INHERITED(SkIRect::MakeWH(width, height)) {}

SkNoDrawCanvas::SkNoDrawCanvas(const SkIRect& bounds) : INHERITED(bounds) {}

SkCanvas::SaveLayerStrategy SkNoDrawCanvas::getSaveLayerStrategy(const SaveLayerRec& rec) {
    (void)this->INHERITED::getSaveLayerStrategy(rec);
    return kNoLayer_SaveLayerStrategy;
}

bool SkNoDrawCanvas::onDoSaveBehind(const SkRect*) { return false; }

static_assert((int)SkRegion::kDifference_Op == (int)SkClipOp::kDifference, "");
static_assert((int)SkRegion::kIntersect_Op == (int)SkClipOp::kIntersect, "");

SkRasterHandleAllocator::Handle SkCanvas::accessTopRasterHandle() const {
    const SkDevice* dev = this->topDevice();
    if (fAllocator) {
        SkRasterHandleAllocator::Handle handle = dev->getRasterHandle();
        SkIRect clip = dev->devClipBounds();
        if (!clip.intersect({0, 0, dev->width(), dev->height()})) {
            clip.setEmpty();
        }
        fAllocator->updateHandle(handle, dev->localToDevice(), clip);
        return handle;
    }
    return nullptr;
}

static bool install_bm(SkBitmap* bm, const SkImageInfo& info,
                       const SkRasterHandleAllocator::Rec& rec) {
    return bm->installPixels(info, rec.fPixels, rec.fRowBytes, rec.fReleaseProc, rec.fReleaseCtx);
}

SkRasterHandleAllocator::Handle SkRasterHandleAllocator::allocBitmap(const SkImageInfo& info,
                                                                     SkBitmap* bm) {
    SkRasterHandleAllocator::Rec rec;
    if (!this->allocHandle(info, &rec) || !install_bm(bm, info, rec)) {
        return nullptr;
    }
    return rec.fHandle;
}

std::unique_ptr<SkCanvas> SkRasterHandleAllocator::MakeCanvas(
        std::unique_ptr<SkRasterHandleAllocator> alloc,
        const SkImageInfo& info, const Rec* rec,
        const SkSurfaceProps* props) {
    if (!alloc || !SkSurfaceValidateRasterInfo(info, rec ? rec->fRowBytes : kIgnoreRowBytesValue)) {
        return nullptr;
    }

    SkBitmap bm;
    Handle hndl;
    if (rec) {
        hndl = install_bm(&bm, info, *rec) ? rec->fHandle : nullptr;
    } else {
        hndl = alloc->allocBitmap(info, &bm);
    }
    return hndl ? std::unique_ptr<SkCanvas>(new SkCanvas(bm, std::move(alloc), hndl, props))
                : nullptr;
}

// ============================================================================
// SkCanvas_Raster.cpp
// ============================================================================

SkCanvas::SkCanvas(const SkBitmap& bitmap, const SkSurfaceProps& props)
        : fMCStack(sizeof(MCRec), fMCRecStorage, sizeof(fMCRecStorage)), fProps(props) {
    this->init(sk_make_sp<SkBitmapDevice>(bitmap, fProps));
}

SkCanvas::SkCanvas(const SkBitmap& bitmap,
                   std::unique_ptr<SkRasterHandleAllocator> alloc,
                   SkRasterHandleAllocator::Handle hndl,
                   const SkSurfaceProps* props)
        : fMCStack(sizeof(MCRec), fMCRecStorage, sizeof(fMCRecStorage))
        , fProps(SkSurfacePropsCopyOrDefault(props))
        , fAllocator(std::move(alloc)) {
    this->init(sk_make_sp<SkBitmapDevice>(bitmap, fProps, hndl));
}

SkCanvas::SkCanvas(const SkBitmap& bitmap) : SkCanvas(bitmap, nullptr, nullptr, nullptr) {}

#if defined(SK_BUILD_FOR_ANDROID_FRAMEWORK)
SkCanvas::SkCanvas(const SkBitmap& bitmap, ColorBehavior)
        : fMCStack(sizeof(MCRec), fMCRecStorage, sizeof(fMCRecStorage)) {
    SkBitmap tmp(bitmap);
    *const_cast<SkImageInfo*>(&tmp.info()) = tmp.info().makeColorSpace(nullptr);
    this->init(sk_make_sp<SkBitmapDevice>(tmp, fProps));
}
#endif

// ============================================================================
// SkCanvasPriv.cpp
// ============================================================================

SkAutoCanvasMatrixPaint::SkAutoCanvasMatrixPaint(SkCanvas* canvas, const SkMatrix* matrix,
                                                 const SkPaint* paint, const SkRect& bounds)
        : fCanvas(canvas)
        , fSaveCount(canvas->getSaveCount()) {
    if (paint) {
        SkRect newBounds = bounds;
        if (matrix) {
            matrix->mapRect(&newBounds);
        }
        canvas->saveLayer(&newBounds, paint);
    } else if (matrix) {
        canvas->save();
    }

    if (matrix) {
        canvas->concat(*matrix);
    }
}

SkAutoCanvasMatrixPaint::~SkAutoCanvasMatrixPaint() {
    fCanvas->restoreToCount(fSaveCount);
}

bool SkCanvasPriv::ReadLattice(SkReadBuffer& buffer, SkCanvas::Lattice* lattice) {
    lattice->fXCount = buffer.readInt();
    if (lattice->fXCount < 0) {
        return false;
    }
    lattice->fXDivs = buffer.skipT<int32_t>(lattice->fXCount);
    lattice->fYCount = buffer.readInt();
    if (lattice->fYCount < 0) {
        return false;
    }
    lattice->fYDivs = buffer.skipT<int32_t>(lattice->fYCount);
    int flagCount = buffer.readInt();
    lattice->fRectTypes = nullptr;
    lattice->fColors = nullptr;
    if (flagCount) {
        if (!skia_rs_canvas_lattice_validate_flag_count(
                    lattice->fXCount, lattice->fYCount, flagCount)) {
            return false;
        }
        lattice->fRectTypes = buffer.skipT<SkCanvas::Lattice::RectType>(flagCount);
        lattice->fColors = buffer.skipT<SkColor>(flagCount);
    }
    lattice->fBounds = buffer.skipT<SkIRect>();
    return buffer.isValid();
}

size_t SkCanvasPriv::WriteLattice(void* buffer, const SkCanvas::Lattice& lattice) {
    int32_t flagCount = 0;
    const size_t size = skia_rs_canvas_lattice_serialized_size(
            lattice.fXCount, lattice.fYCount, lattice.fRectTypes != nullptr, &flagCount);

    if (buffer) {
        SkWriter32 writer(buffer, size);
        writer.write32(lattice.fXCount);
        writer.write(lattice.fXDivs, lattice.fXCount * sizeof(uint32_t));
        writer.write32(lattice.fYCount);
        writer.write(lattice.fYDivs, lattice.fYCount * sizeof(uint32_t));
        writer.write32(flagCount);
        writer.writePad(lattice.fRectTypes, flagCount * sizeof(uint8_t));
        writer.write(lattice.fColors, flagCount * sizeof(SkColor));
        SkASSERT(lattice.fBounds);
        writer.write(lattice.fBounds, sizeof(SkIRect));
        SkASSERT(writer.bytesWritten() == size);
    }
    return size;
}

void SkCanvasPriv::WriteLattice(SkWriteBuffer& buffer, const SkCanvas::Lattice& lattice) {
    const size_t size = WriteLattice(nullptr, lattice);
    SkAutoSMalloc<1024> storage(size);
    WriteLattice(storage.get(), lattice);
    buffer.writePad32(storage.get(), size);
}

void SkCanvasPriv::GetDstClipAndMatrixCounts(const SkCanvas::ImageSetEntry set[], int count,
                                             int* totalDstClipCount, int* totalMatrixCount) {
    if (count <= 0) {
        *totalDstClipCount = 0;
        *totalMatrixCount = 0;
        return;
    }
    skia_private::AutoSTArray<16, uint8_t> hasClips(count);
    skia_private::AutoSTArray<16, int32_t> matrixIndices(count);
    for (int i = 0; i < count; ++i) {
        hasClips[i] = set[i].fHasClip ? 1 : 0;
        matrixIndices[i] = set[i].fMatrixIndex;
    }
    SkiaRsDstClipMatrixCounts res = skia_rs_canvas_dst_clip_and_matrix_counts(
            hasClips.data(), matrixIndices.data(), count);
    *totalDstClipCount = res.total_dst_clip_count;
    *totalMatrixCount = res.total_matrix_count;
}

bool SkCanvasPriv::ImageToColorFilter(SkPaint* paint) {
    SkASSERT(SkToBool(paint) && paint->getImageFilter());

    if (paint->getMaskFilter()) {
        return false;
    }

    SkColorFilter* imgCFPtr;
    if (!paint->getImageFilter()->asAColorFilter(&imgCFPtr)) {
        return false;
    }
    sk_sp<SkColorFilter> imgCF(imgCFPtr);

    SkColorFilter* paintCF = paint->getColorFilter();
    if (paintCF) {
        imgCF = imgCF->makeComposed(sk_ref_sp(paintCF));
    }

    paint->setColorFilter(std::move(imgCF));
    paint->setImageFilter(nullptr);
    return true;
}

AutoLayerForImageFilter::AutoLayerForImageFilter(SkCanvas* canvas,
                                                 const SkPaint& paint,
                                                 const SkRect* rawBounds,
                                                 bool skipMaskFilterLayer)
        : fPaint(paint)
        , fCanvas(canvas)
        , fTempLayersForFilters(0) {
    SkDEBUGCODE(fSaveCount = canvas->getSaveCount();)

    if (fPaint.getImageFilter() && !SkCanvasPriv::ImageToColorFilter(&fPaint)) {
        this->addImageFilterLayer(rawBounds);
    }

    if (fPaint.getMaskFilter() && !skipMaskFilterLayer) {
        this->addMaskFilterLayer(rawBounds);
    }
}

AutoLayerForImageFilter::~AutoLayerForImageFilter() {
    for (int i = 0; i < fTempLayersForFilters; ++i) {
        fCanvas->fSaveCount -= 1;
        fCanvas->internalRestore();
    }
    SkASSERT(fSaveCount < 0 || fCanvas->getSaveCount() == fSaveCount);
}

AutoLayerForImageFilter::AutoLayerForImageFilter(AutoLayerForImageFilter&& other) {
    *this = std::move(other);
}

AutoLayerForImageFilter& AutoLayerForImageFilter::operator=(AutoLayerForImageFilter&& other) {
    fPaint = std::move(other.fPaint);
    fCanvas = other.fCanvas;
    fTempLayersForFilters = other.fTempLayersForFilters;
    SkDEBUGCODE(fSaveCount = other.fSaveCount;)

    other.fTempLayersForFilters = 0;
    SkDEBUGCODE(other.fSaveCount = -1;)

    return *this;
}

void AutoLayerForImageFilter::addImageFilterLayer(const SkRect* drawBounds) {
    SkASSERT(fPaint.getImageFilter());

    SkPaint restorePaint;
    restorePaint.setImageFilter(fPaint.refImageFilter());
    restorePaint.setBlender(fPaint.refBlender());

    fPaint.setImageFilter(nullptr);
    fPaint.setBlendMode(SkBlendMode::kSrcOver);

    this->addLayer(restorePaint, drawBounds, /*coverageOnly=*/false);
}

void AutoLayerForImageFilter::addMaskFilterLayer(const SkRect* drawBounds) {
    SkASSERT(fPaint.getMaskFilter());
    SkASSERT(!fPaint.getImageFilter());

    auto [maskFilterAsImageFilter, appliesShading] =
            as_MFB(fPaint.getMaskFilter())->asImageFilter(fCanvas->getTotalMatrix(), fPaint);
    if (!maskFilterAsImageFilter) {
        return;
    }

    SkPaint restorePaint;
    if (!appliesShading) {
        restorePaint.setColor4f(fPaint.getColor4f());
        restorePaint.setShader(fPaint.refShader());
        restorePaint.setColorFilter(fPaint.refColorFilter());
        restorePaint.setDither(fPaint.isDither());
    }
    restorePaint.setBlender(fPaint.refBlender());
    restorePaint.setImageFilter(maskFilterAsImageFilter);

    fPaint.setColor4f(SkColors::kWhite);
    fPaint.setShader(nullptr);
    fPaint.setColorFilter(nullptr);
    fPaint.setMaskFilter(nullptr);
    fPaint.setDither(false);
    fPaint.setBlendMode(SkBlendMode::kSrcOver);

    this->addLayer(restorePaint, drawBounds, /*coverageOnly=*/!appliesShading);
}

void AutoLayerForImageFilter::addLayer(const SkPaint& restorePaint,
                                       const SkRect* drawBounds,
                                       bool coverageOnly) {
    SkRect storage;
    const SkRect* contentBounds = nullptr;
    if (drawBounds && fPaint.canComputeFastBounds()) {
        contentBounds = &fPaint.computeFastBounds(*drawBounds, &storage);
    }

    fCanvas->fSaveCount += 1;
    fCanvas->internalSaveLayer(SkCanvas::SaveLayerRec(contentBounds, &restorePaint),
                               SkCanvas::kFullLayer_SaveLayerStrategy,
                               coverageOnly);
    fTempLayersForFilters += 1;
}

// ============================================================================
// SkOverdrawCanvas.cpp
// ============================================================================

SkOverdrawCanvas::SkOverdrawCanvas(SkCanvas* canvas)
        : INHERITED(canvas->onImageInfo().width(), canvas->onImageInfo().height()) {
    this->addCanvas(canvas);

    float incrementAlpha[20];
    skia_rs_canvas_overdraw_alpha_matrix(incrementAlpha);

    fPaint.setAntiAlias(false);
    fPaint.setBlendMode(SkBlendMode::kPlus);
    fPaint.setColorFilter(SkColorFilters::Matrix(incrementAlpha));
}

namespace {
class TextDevice : public SkNoPixelsDevice, public skcpu::BitmapDevicePainter {
public:
    SK_ONLY_STACK_ALLOCATABLE;

    TextDevice(SkCanvas* overdrawCanvas, const SkSurfaceProps& props)
            : SkNoPixelsDevice{SkIRect::MakeWH(32767, 32767), props}
            , fOverdrawCanvas{overdrawCanvas}
            , fPainter{props, kN32_SkColorType, nullptr} {}

    void paintMasks(SkZip<const SkGlyph*, SkPoint> accepted, const SkPaint&) const override {
        for (auto [glyph, pos] : accepted) {
            SkMask mask = glyph->mask(pos);
            fOverdrawCanvas->save();
            fOverdrawCanvas->resetMatrix();
            fOverdrawCanvas->drawRect(SkRect::Make(mask.fBounds), SkPaint());
            fOverdrawCanvas->restore();
        }
    }

    void drawBitmap(const SkBitmap&, const SkMatrix&, const SkRect*,
                    const SkSamplingOptions&, const SkPaint&, sk_sp<SkMipmap>) const override {}

    void onDrawGlyphRunList(SkCanvas* canvas,
                            const sktext::GlyphRunList& glyphRunList,
                            const SkPaint& paint) override {
        SkASSERT(!glyphRunList.hasRSXForm());
        fPainter.drawForBitmapDevice(
                canvas, this, glyphRunList, paint, fOverdrawCanvas->getTotalMatrix());
    }

private:
    SkCanvas* const fOverdrawCanvas;
    skcpu::GlyphRunListPainter fPainter;
};
}  // namespace

void SkOverdrawCanvas::onDrawTextBlob(
        const SkTextBlob* blob, SkScalar x, SkScalar y, const SkPaint& paint) {
    sktext::GlyphRunBuilder b;
    auto glyphRunList = b.blobToGlyphRunList(*blob, {x, y});
    this->onDrawGlyphRunList(glyphRunList, paint);
}

void SkOverdrawCanvas::onDrawGlyphRunList(const sktext::GlyphRunList& glyphRunList,
                                          const SkPaint& paint) {
    SkSurfaceProps props;
    this->getProps(&props);
    TextDevice device{this, props};
    device.drawGlyphRunList(this, glyphRunList, paint);
}

void SkOverdrawCanvas::onDrawPatch(const SkPoint cubics[12], const SkColor colors[4],
                                   const SkPoint texCoords[4], SkBlendMode blendMode,
                                   const SkPaint&) {
    fList[0]->onDrawPatch(cubics, colors, texCoords, blendMode, fPaint);
}

void SkOverdrawCanvas::onDrawPaint(const SkPaint& paint) {
    if (!skia_rs_canvas_overdraw_is_clear_paint(
                paint.getColor(), paint.getColorFilter() != nullptr, paint.getShader() != nullptr)) {
        fList[0]->onDrawPaint(this->overdrawPaint(paint));
    }
}

void SkOverdrawCanvas::onDrawBehind(const SkPaint& paint) {
    fList[0]->onDrawBehind(this->overdrawPaint(paint));
}

void SkOverdrawCanvas::onDrawRect(const SkRect& rect, const SkPaint& paint) {
    fList[0]->onDrawRect(rect, this->overdrawPaint(paint));
}

void SkOverdrawCanvas::onDrawRegion(const SkRegion& region, const SkPaint& paint) {
    fList[0]->onDrawRegion(region, this->overdrawPaint(paint));
}

void SkOverdrawCanvas::onDrawOval(const SkRect& oval, const SkPaint& paint) {
    fList[0]->onDrawOval(oval, this->overdrawPaint(paint));
}

void SkOverdrawCanvas::onDrawArc(const SkRect& arc, SkScalar startAngle, SkScalar sweepAngle,
                                 bool useCenter, const SkPaint& paint) {
    fList[0]->onDrawArc(arc, startAngle, sweepAngle, useCenter, this->overdrawPaint(paint));
}

void SkOverdrawCanvas::onDrawDRRect(const SkRRect& outer, const SkRRect& inner,
                                    const SkPaint& paint) {
    fList[0]->onDrawDRRect(outer, inner, this->overdrawPaint(paint));
}

void SkOverdrawCanvas::onDrawRRect(const SkRRect& rect, const SkPaint& paint) {
    fList[0]->onDrawRRect(rect, this->overdrawPaint(paint));
}

void SkOverdrawCanvas::onDrawPoints(PointMode mode, size_t count, const SkPoint points[],
                                    const SkPaint& paint) {
    fList[0]->onDrawPoints(mode, count, points, this->overdrawPaint(paint));
}

void SkOverdrawCanvas::onDrawVerticesObject(const SkVertices* vertices,
                                            SkBlendMode blendMode, const SkPaint& paint) {
    fList[0]->onDrawVerticesObject(vertices, blendMode, this->overdrawPaint(paint));
}

void SkOverdrawCanvas::onDrawAtlas2(const SkImage* image, const SkRSXform xform[],
                                    const SkRect texs[], const SkColor colors[], int count,
                                    SkBlendMode mode, const SkSamplingOptions& sampling,
                                    const SkRect* cull, const SkPaint* paint) {
    SkPaint* paintPtr = &fPaint;
    SkPaint storage;
    if (paint) {
        storage = this->overdrawPaint(*paint);
        paintPtr = &storage;
    }
    fList[0]->onDrawAtlas2(image, xform, texs, colors, count, mode, sampling, cull, paintPtr);
}

void SkOverdrawCanvas::onDrawPath(const SkPath& path, const SkPaint&) {
    fList[0]->onDrawPath(path, fPaint);
}

void SkOverdrawCanvas::onDrawImage2(const SkImage* image, SkScalar x, SkScalar y,
                                    const SkSamplingOptions&, const SkPaint*) {
    fList[0]->onDrawRect(SkRect::MakeXYWH(x, y, image->width(), image->height()), fPaint);
}

void SkOverdrawCanvas::onDrawImageRect2(const SkImage*, const SkRect&, const SkRect& dst,
                                        const SkSamplingOptions&, const SkPaint*,
                                        SrcRectConstraint) {
    fList[0]->onDrawRect(dst, fPaint);
}

void SkOverdrawCanvas::onDrawImageLattice2(const SkImage* image, const Lattice& lattice,
                                           const SkRect& dst, SkFilterMode, const SkPaint*) {
    SkIRect bounds;
    Lattice latticePlusBounds = lattice;
    if (!latticePlusBounds.fBounds) {
        bounds = SkIRect::MakeWH(image->width(), image->height());
        latticePlusBounds.fBounds = &bounds;
    }

    if (SkLatticeIter::Valid(image->width(), image->height(), latticePlusBounds)) {
        SkLatticeIter iter(latticePlusBounds, dst);
        SkRect ignored, iterDst;
        while (iter.next(&ignored, &iterDst)) {
            fList[0]->onDrawRect(iterDst, fPaint);
        }
    } else {
        fList[0]->onDrawRect(dst, fPaint);
    }
}

void SkOverdrawCanvas::onDrawDrawable(SkDrawable* drawable, const SkMatrix* matrix) {
    drawable->draw(this, matrix);
}

void SkOverdrawCanvas::onDrawPicture(const SkPicture*, const SkMatrix*, const SkPaint*) {
    SkASSERT(false);
}

void SkOverdrawCanvas::onDrawAnnotation(const SkRect&, const char[], SkData*) {}

void SkOverdrawCanvas::onDrawShadowRec(const SkPath& path, const SkDrawShadowRec& rec) {
    SkRect bounds;
    SkDrawShadowMetrics::GetLocalBounds(path, rec, this->getTotalMatrix(), &bounds);
    fList[0]->onDrawRect(bounds, fPaint);
}

void SkOverdrawCanvas::onDrawEdgeAAQuad(const SkRect& rect, const SkPoint clip[4],
                                        QuadAAFlags, const SkColor4f&, SkBlendMode) {
    if (clip) {
        fList[0]->onDrawPath(SkPath::Polygon({clip, 4}, true), fPaint);
    } else {
        fList[0]->onDrawRect(rect, fPaint);
    }
}

void SkOverdrawCanvas::onDrawEdgeAAImageSet2(const ImageSetEntry set[], int count,
                                             const SkPoint dstClips[],
                                             const SkMatrix preViewMatrices[],
                                             const SkSamplingOptions&,
                                             const SkPaint*,
                                             SrcRectConstraint) {
    int clipIndex = 0;
    for (int i = 0; i < count; ++i) {
        if (set[i].fMatrixIndex >= 0) {
            fList[0]->save();
            fList[0]->concat(preViewMatrices[set[i].fMatrixIndex]);
        }
        if (set[i].fHasClip) {
            fList[0]->onDrawPath(SkPath::Polygon({dstClips + clipIndex, 4}, true), fPaint);
            clipIndex += 4;
        } else {
            fList[0]->onDrawRect(set[i].fDstRect, fPaint);
        }
        if (set[i].fMatrixIndex >= 0) {
            fList[0]->restore();
        }
    }
}

inline SkPaint SkOverdrawCanvas::overdrawPaint(const SkPaint& paint) {
    SkPaint newPaint = fPaint;
    newPaint.setStyle(paint.getStyle());
    newPaint.setStrokeWidth(paint.getStrokeWidth());
    return newPaint;
}

#undef RETURN_ON_NULL
#undef RETURN_ON_FALSE
