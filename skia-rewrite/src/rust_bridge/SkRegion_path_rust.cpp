/*
 * Safe Rust bridge for src/core/SkRegion_path.cpp (Layer 2.3 skia-rs-region).
 */

#include "include/core/SkPath.h"
#include "include/core/SkPathBuilder.h"
#include "include/core/SkPathTypes.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRect.h"
#include "include/core/SkRegion.h"
#include "include/core/SkScalar.h"
#include "include/core/SkSpan.h"
#include "include/private/SkAssert.h"
#include "include/private/SkOnlyStackAllocatable.h"
#include "src/core/SkSafeMath.h"
#include "src/core/SkBlitter.h"
#include "src/core/SkPathPriv.h"
#include "src/core/SkRegionPriv.h"
#include "src/core/SkScan.h"

#include <algorithm>
#include <cstddef>
#include <cstdint>

extern "C" {
struct SkiaRsRegionBuilder;
struct SkiaRsPathCmdBuf;

struct SkiaRsPathTileGrid {
    SkIRect intersection;
    int32_t tile_size;
    int32_t tiles_x;
    int32_t tiles_y;
};
static_assert(sizeof(SkiaRsPathTileGrid) == 28);

struct SkiaRsPathCmd {
    uint8_t verb;
    float x;
    float y;
};
static_assert(sizeof(SkiaRsPathCmd) == 12);

uint8_t skia_rs_region_verb_to_initial_last_index(uint8_t verb);
uint32_t skia_rs_region_verb_to_max_edges(uint8_t verb);
bool skia_rs_region_compute_path_tile_grid(
        const SkIRect* path_bounds, const SkIRect* clip_bounds, SkiaRsPathTileGrid* out_grid);

SkiaRsRegionBuilder* skia_rs_region_builder_new(
        int32_t max_height, int32_t max_transitions, bool path_is_inverse);
void skia_rs_region_builder_free(SkiaRsRegionBuilder* b);
void skia_rs_region_builder_blit_h(SkiaRsRegionBuilder* b, int32_t x, int32_t y, int32_t width);
void skia_rs_region_builder_done(SkiaRsRegionBuilder* b);
int32_t skia_rs_region_builder_compute_run_count(const SkiaRsRegionBuilder* b);
bool skia_rs_region_builder_to_rect(const SkiaRsRegionBuilder* b, SkIRect* out_rect);
void skia_rs_region_builder_copy_to_runs(
        const SkiaRsRegionBuilder* b, int32_t* dst, size_t len);

SkiaRsPathCmdBuf* skia_rs_region_extract_boundary_path(const int32_t* runs, size_t run_count);
const SkiaRsPathCmd* skia_rs_region_path_cmd_buf_data(
        const SkiaRsPathCmdBuf* buf, size_t* out_len);
void skia_rs_region_path_cmd_buf_free(SkiaRsPathCmdBuf* buf);
}  // extern "C"

namespace {

class RustRgnBlitter final : public SkBlitter {
public:
    SK_ONLY_STACK_ALLOCATABLE;

    RustRgnBlitter() = default;
    ~RustRgnBlitter() override {
        if (fBuilder) {
            skia_rs_region_builder_free(fBuilder);
        }
    }

    bool init(int maxHeight, int maxTransitions, bool pathIsInverse) {
        fBuilder = skia_rs_region_builder_new(maxHeight, maxTransitions, pathIsInverse);
        return fBuilder != nullptr;
    }

    void done() {
        skia_rs_region_builder_done(fBuilder);
    }

    int computeRunCount() const {
        return skia_rs_region_builder_compute_run_count(fBuilder);
    }

    bool toRect(SkIRect* out) const {
        return skia_rs_region_builder_to_rect(fBuilder, out);
    }

    void copyToRgn(SkRegionPriv::RunType runs[], int count) const {
        skia_rs_region_builder_copy_to_runs(fBuilder, runs, static_cast<size_t>(count));
    }

    void blitH(int x, int y, int width) override {
        skia_rs_region_builder_blit_h(fBuilder, x, y, width);
    }

    void blitAntiH(int, int, const SkAlpha[], const int16_t[]) override {
        SkDEBUGFAIL("blitAntiH not implemented");
    }

private:
    SkiaRsRegionBuilder* fBuilder = nullptr;
};

static int count_path_runtype_values(const SkPath& path, int* itop, int* ibot) {
    SkSafeMath safe;
    int maxEdges = 0;
    SkScalar top = SkIntToScalar(SK_MaxS16);
    SkScalar bot = SkIntToScalar(SK_MinS16);

    SkPath::Iter iter(path, true);
    while (auto rec = iter.next()) {
        const SkPathVerb verb = rec->fVerb;
        const SkSpan<const SkPoint> pts = rec->fPoints;
        const uint8_t v = static_cast<uint8_t>(verb);

        maxEdges = safe.addInt(maxEdges, static_cast<int>(skia_rs_region_verb_to_max_edges(v)));

        int lastIndex = static_cast<int>(skia_rs_region_verb_to_initial_last_index(v));
        if (lastIndex > 0) {
            for (int i = 1; i <= lastIndex; i++) {
                if (top > pts[i].fY) {
                    top = pts[i].fY;
                } else if (bot < pts[i].fY) {
                    bot = pts[i].fY;
                }
            }
        } else if (SkPathVerb::kMove == verb) {
            if (top > pts[0].fY) {
                top = pts[0].fY;
            } else if (bot < pts[0].fY) {
                bot = pts[0].fY;
            }
        }
    }
    if (!safe) {
        return -1;
    }
    if (0 == maxEdges) {
        return 0;
    }

    SkASSERT(top <= bot);
    *itop = SkScalarRoundToInt(top);
    *ibot = SkScalarRoundToInt(bot);
    return maxEdges;
}

static bool check_inverse_on_empty_return(SkRegion* dst, const SkPath& path, const SkRegion& clip) {
    if (path.isInverseFillType()) {
        return dst->set(clip);
    }
    return dst->setEmpty();
}

}  // namespace

bool SkRegion::setPath(const SkPath& path, const SkRegion& clip) {
    SkDEBUGCODE(SkRegionPriv::Validate(*this));

    const auto raw = SkPathPriv::Raw(path, SkResolveConvexity::kYes);

    if (clip.isEmpty() || !raw.has_value() || path.isEmpty()) {
        return check_inverse_on_empty_return(this, path, clip);
    }

    const SkIRect clipBounds = clip.getBounds();
    if (clip.isComplex()) {
        if (!this->setPath(path, SkRegion(clipBounds))) {
            return false;
        }
        return this->op(clip, kIntersect_Op);
    }

    if (SkScan::PathRequiresTiling(clipBounds)) {
        const SkIRect pathBounds = path.getBounds().roundOut();
        this->setEmpty();

        SkiaRsPathTileGrid grid;
        if (!skia_rs_region_compute_path_tile_grid(&pathBounds, &clipBounds, &grid)) {
            return false;
        }

        const SkIRect& intersection = grid.intersection;
        const int64_t kTileSize = grid.tile_size;
        for (int64_t top = intersection.fTop; top < intersection.fBottom; top += kTileSize) {
            int64_t bot = std::min(top + kTileSize, static_cast<int64_t>(intersection.fBottom));
            for (int64_t left = intersection.fLeft; left < intersection.fRight; left += kTileSize) {
                int64_t right = std::min(left + kTileSize, static_cast<int64_t>(intersection.fRight));

                SkIRect tileClipBounds = {
                    static_cast<int>(left),
                    static_cast<int>(top),
                    static_cast<int>(right),
                    static_cast<int>(bot),
                };
                tileClipBounds.offset(static_cast<int>(-left), static_cast<int>(-top));
                SkASSERT(!SkScan::PathRequiresTiling(tileClipBounds));
                SkRegion tile;
                if (auto newpath = path.tryMakeOffset(static_cast<SkScalar>(-left),
                                                      static_cast<SkScalar>(-top))) {
                    tile.setPath(*newpath, SkRegion(tileClipBounds));
                    tile.translate(static_cast<int>(left), static_cast<int>(top));
                    this->op(tile, kUnion_Op);
                } else {
                    return false;
                }
            }
        }
        return this->op(clip, kIntersect_Op);
    }

    int pathTop, pathBot;
    int pathTransitions = count_path_runtype_values(path, &pathTop, &pathBot);
    if (pathTransitions < 0) {
        return this->setEmpty();
    }
    if (0 == pathTransitions) {
        return check_inverse_on_empty_return(this, path, clip);
    }

    int clipTop, clipBot;
    int clipTransitions = clip.count_runtype_values(&clipTop, &clipBot);

    int top = std::max(pathTop, clipTop);
    int bot = std::min(pathBot, clipBot);
    if (top >= bot) {
        return check_inverse_on_empty_return(this, path, clip);
    }

    RustRgnBlitter builder;
    if (!builder.init(bot - top,
                      std::max(pathTransitions, clipTransitions),
                      path.isInverseFillType())) {
        return this->setEmpty();
    }

    SkScan::FillPath(*raw, clip, &builder);
    builder.done();

    int count = builder.computeRunCount();
    if (count == 0) {
        return this->setEmpty();
    } else if (count == kRectRegionRuns) {
        SkIRect r;
        builder.toRect(&r);
        this->setRect(r);
    } else {
        SkRegion tmp;
        tmp.fRunHead = RunHead::Alloc(count);
        builder.copyToRgn(tmp.fRunHead->writable_runs(), count);
        tmp.fRunHead->computeRunBounds(&tmp.fBounds);
        this->swap(tmp);
    }
    SkDEBUGCODE(SkRegionPriv::Validate(*this));
    return true;
}

bool SkRegion::addBoundaryPath(SkPathBuilder* builder) const {
    SkASSERT(builder);
    if (this->isEmpty()) {
        return false;
    }

    const SkIRect& bounds = this->getBounds();
    if (this->isRect()) {
        SkRect r;
        r.set(bounds);
        builder->addRect(r);
        return true;
    }

    SkiaRsPathCmdBuf* buf = skia_rs_region_extract_boundary_path(
            fRunHead->readonly_runs(), static_cast<size_t>(fRunHead->fRunCount));
    size_t len = 0;
    const SkiaRsPathCmd* cmds = skia_rs_region_path_cmd_buf_data(buf, &len);
    builder->incReserve(static_cast<int>(len));
    for (size_t i = 0; i < len; ++i) {
        switch (cmds[i].verb) {
            case 0:
                builder->moveTo(cmds[i].x, cmds[i].y);
                break;
            case 1:
                builder->lineTo(cmds[i].x, cmds[i].y);
                break;
            case 2:
                builder->close();
                break;
            default:
                break;
        }
    }
    skia_rs_region_path_cmd_buf_free(buf);
    return true;
}

SkPath SkRegion::getBoundaryPath() const {
    SkPathBuilder builder;
    (void)this->addBoundaryPath(&builder);
    return builder.detach();
}
