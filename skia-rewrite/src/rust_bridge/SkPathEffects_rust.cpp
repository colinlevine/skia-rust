/*
 * Safe Rust bridge for:
 *   src/effects/Sk1DPathEffect.cpp
 *   src/effects/Sk2DPathEffect.cpp
 *   src/effects/SkCornerPathEffect.cpp
 *   src/effects/SkDashPathEffect.cpp
 *   src/effects/SkDiscretePathEffect.cpp
 *   src/effects/SkTrimPathEffect.cpp
 * (Phase 1 Layer 4.3 skia-rs-effects, GN target :skia).
 */

#include "include/core/SkFlattenable.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPath.h"
#include "include/core/SkPathBuilder.h"
#include "include/core/SkPathEffect.h"
#include "include/core/SkPathMeasure.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkRegion.h"
#include "include/core/SkScalar.h"
#include "include/core/SkStrokeRec.h"
#include "include/core/SkTypes.h"
#include "include/effects/Sk1DPathEffect.h"
#include "include/effects/Sk2DPathEffect.h"
#include "include/effects/SkCornerPathEffect.h"
#include "include/effects/SkDashPathEffect.h"
#include "include/effects/SkDiscretePathEffect.h"
#include "include/effects/SkTrimPathEffect.h"
#include "include/private/SkAlign.h"
#include "include/private/SkTemplates.h"
#include "src/core/SkPathEffectBase.h"
#include "src/core/SkReadBuffer.h"
#include "src/core/SkWriteBuffer.h"
#include "src/effects/SkDashImpl.h"
#include "src/effects/SkTrimPE.h"
#include "src/utils/SkDashPathPriv.h"

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <optional>
#include <utility>

using namespace skia_private;

namespace {

struct RsCornerPathState;
struct RsDashPointsResult;

struct RsCornerOutCmd {
    uint8_t verb;
    uint8_t pad[3];
    SkPoint pts[3];
    float weight;
};

struct RsDiscreteContourPlan {
    bool copy_verbatim;
    uint8_t pad[3];
    int32_t n;
    float delta;
    float initial_distance;
};

struct RsTrimSpanPlan {
    bool has_first_span;
    bool has_second_span;
    bool is_inverted;
    uint8_t pad;
    float first_start;
    float first_stop;
    float second_start;
    float second_stop;
};

static_assert(sizeof(SkMatrix) == 40);
static_assert(sizeof(SkPoint) == 8);
static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(RsCornerOutCmd) == 32);
static_assert(sizeof(RsDiscreteContourPlan) == 16);
static_assert(sizeof(RsTrimSpanPlan) == 20);
static_assert(static_cast<uint8_t>(SkPathVerb::kMove) == 0);
static_assert(static_cast<uint8_t>(SkPathVerb::kLine) == 1);
static_assert(static_cast<uint8_t>(SkPathVerb::kQuad) == 2);
static_assert(static_cast<uint8_t>(SkPathVerb::kConic) == 3);
static_assert(static_cast<uint8_t>(SkPathVerb::kCubic) == 4);
static_assert(static_cast<uint8_t>(SkPathVerb::kClose) == 5);

}  // namespace

extern "C" {
bool skia_rs_path_1d_is_valid(float advance, float phase, bool path_is_empty);
float skia_rs_path_1d_initial_offset(float advance, float phase);
SkPoint skia_rs_path_1d_morph_point(float sx, float sy, SkPoint pos, SkVector tangent);
SkPoint skia_rs_path_1d_line_midpoint(SkPoint p0, SkPoint p1);

bool skia_rs_line_2d_is_valid(float width);
bool skia_rs_line_2d_span_endpoints(int32_t u,
                                    int32_t v,
                                    int32_t ucount,
                                    const SkMatrix* mat,
                                    SkPoint* out_pts);
SkPoint skia_rs_path_2d_cell_location(int32_t x, int32_t y, const SkMatrix* mat);

bool skia_rs_corner_is_valid(float radius);
RsCornerPathState* skia_rs_corner_state_new(float radius);
size_t skia_rs_corner_state_process(RsCornerPathState* state,
                                    uint8_t verb,
                                    const SkPoint* pts,
                                    size_t pt_count,
                                    float conic_weight,
                                    bool is_closed_contour,
                                    RsCornerOutCmd* out_cmds);
bool skia_rs_corner_state_finish(const RsCornerPathState* state, SkPoint* out_pt);
void skia_rs_corner_state_free(RsCornerPathState* state);

RsDashPointsResult* skia_rs_dash_as_points(const float* intervals,
                                           size_t interval_count,
                                           float initial_dash_length,
                                           size_t initial_dash_index,
                                           float interval_length,
                                           const SkPoint* line_pts,
                                           float stroke_width,
                                           int32_t cap,
                                           bool is_miter_join,
                                           float miter,
                                           const SkMatrix* matrix,
                                           const SkRect* cull_rect,
                                           bool populate_results);
void skia_rs_dash_points_extract(const RsDashPointsResult* res,
                                 SkPoint* out_size,
                                 uint32_t* out_flags,
                                 const SkPoint** out_pts_ptr,
                                 size_t* out_pts_len,
                                 bool* out_has_first,
                                 SkRect* out_first_rect,
                                 bool* out_has_last,
                                 SkRect* out_last_rect);
void skia_rs_dash_points_free(RsDashPointsResult* res);

bool skia_rs_discrete_is_valid(float seg_length, float deviation);
uint32_t skia_rs_discrete_rng_init(uint32_t seed_assist, float first_contour_length);
float skia_rs_discrete_rng_next(uint32_t* seed);
SkPoint skia_rs_discrete_perturb(SkPoint p, SkVector tangent, float scale);
RsDiscreteContourPlan skia_rs_discrete_contour_plan(float seg_length,
                                                    float length,
                                                    bool do_fill,
                                                    bool is_closed);
SkRect skia_rs_discrete_compute_fast_bounds(SkRect bounds, float deviation);

bool skia_rs_trim_validate_and_pin(float start_t,
                                   float stop_t,
                                   uint32_t mode,
                                   float* out_start,
                                   float* out_stop);
RsTrimSpanPlan skia_rs_trim_compute_spans(float total_len,
                                          float start_t,
                                          float stop_t,
                                          uint32_t mode);
}  // extern "C"

// ============================================================================
// Sk1DPathEffect (src/effects/Sk1DPathEffect.cpp)
// ============================================================================

#define MAX_REASONABLE_ITERATIONS 100000

class Sk1DPathEffect : public SkPathEffectBase {
protected:
    bool onFilterPath(SkPathBuilder* builder, const SkPath& src, SkStrokeRec*, const SkRect*,
                      const SkMatrix&) const override {
        SkPathMeasure meas(src, false);
        do {
            int governor = MAX_REASONABLE_ITERATIONS;
            SkScalar length = meas.getLength();
            SkScalar distance = this->begin(length);
            while (distance < length && --governor >= 0) {
                SkScalar delta = this->next(builder, distance, meas);
                if (delta <= 0) {
                    break;
                }
                distance += delta;
            }
            if (governor < 0) {
                return false;
            }
        } while (meas.nextContour());
        return true;
    }

    virtual SkScalar begin(SkScalar contourLength) const = 0;
    virtual SkScalar next(SkPathBuilder* dst, SkScalar dist, SkPathMeasure&) const = 0;

private:
    bool computeFastBounds(SkRect*) const override { return false; }
};

class SkPath1DPathEffectImpl : public Sk1DPathEffect {
public:
    SkPath1DPathEffectImpl(const SkPath& path, SkScalar advance, SkScalar phase,
                           SkPath1DPathEffect::Style style)
        : fPath(path)
        , fAdvance(advance)
        , fInitialOffset(skia_rs_path_1d_initial_offset(advance, phase))
        , fStyle(style) {
        SkASSERT(advance > 0 && !path.isEmpty());
        SkASSERT(fInitialOffset >= 0);
    }

    bool onFilterPath(SkPathBuilder* builder, const SkPath& src, SkStrokeRec* rec,
                      const SkRect* cullRect, const SkMatrix& ctm) const override {
        rec->setFillStyle();
        return this->INHERITED::onFilterPath(builder, src, rec, cullRect, ctm);
    }

    SkScalar begin(SkScalar) const override { return fInitialOffset; }

    SkScalar next(SkPathBuilder*, SkScalar, SkPathMeasure&) const override;

    static sk_sp<SkFlattenable> CreateProc(SkReadBuffer& buffer) {
        sk_sp<SkFlattenable> result;
        SkScalar advance = buffer.readScalar();
        if (auto path = buffer.readPath()) {
            SkScalar phase = buffer.readScalar();
            SkPath1DPathEffect::Style style = buffer.read32LE(SkPath1DPathEffect::kLastEnum_Style);
            if (buffer.isValid()) {
                result = SkPath1DPathEffect::Make(*path, advance, phase, style);
            }
        }
        return result;
    }

    void flatten(SkWriteBuffer& buffer) const override {
        buffer.writeScalar(fAdvance);
        buffer.writePath(fPath);
        buffer.writeScalar(fInitialOffset);
        buffer.writeUInt(fStyle);
    }

    Factory getFactory() const override { return CreateProc; }
    const char* getTypeName() const override { return "SkPath1DPathEffect"; }

private:
    SkPath fPath;
    SkScalar fAdvance;
    SkScalar fInitialOffset;
    SkPath1DPathEffect::Style fStyle;

    using INHERITED = Sk1DPathEffect;
};

static bool morphpoints(SkSpan<SkPoint> dst, SkSpan<const SkPoint> src,
                        SkPathMeasure& meas, SkScalar dist) {
    SkASSERT(dst.size() >= src.size());
    for (size_t i = 0; i < src.size(); i++) {
        SkPoint pos;
        SkVector tangent;
        if (!meas.getPosTan(dist + src[i].fX, &pos, &tangent)) {
            return false;
        }
        dst[i] = skia_rs_path_1d_morph_point(src[i].fX, src[i].fY, pos, tangent);
    }
    return true;
}

static void morphpath(SkPathBuilder* dst, const SkPath& src, SkPathMeasure& meas, SkScalar dist) {
    SkPath::Iter iter(src, false);
    SkPoint dstP[3], scratch[3];
    while (auto rec = iter.next()) {
        SkSpan<const SkPoint> srcP = rec->fPoints;
        switch (rec->fVerb) {
            case SkPathVerb::kMove:
                if (morphpoints(dstP, srcP, meas, dist)) {
                    dst->moveTo(dstP[0]);
                }
                break;
            case SkPathVerb::kLine:
                scratch[0] = srcP[0];
                scratch[1] = skia_rs_path_1d_line_midpoint(srcP[0], srcP[1]);
                scratch[2] = srcP[1];
                srcP = scratch;
                [[fallthrough]];
            case SkPathVerb::kQuad:
                if (morphpoints(dstP, srcP.subspan(1), meas, dist)) {
                    dst->quadTo(dstP[0], dstP[1]);
                }
                break;
            case SkPathVerb::kConic:
                if (morphpoints(dstP, srcP.subspan(1), meas, dist)) {
                    dst->conicTo(dstP[0], dstP[1], rec->conicWeight());
                }
                break;
            case SkPathVerb::kCubic:
                if (morphpoints(dstP, srcP.subspan(1), meas, dist)) {
                    dst->cubicTo(dstP[0], dstP[1], dstP[2]);
                }
                break;
            case SkPathVerb::kClose:
                dst->close();
                break;
        }
    }
}

SkScalar SkPath1DPathEffectImpl::next(SkPathBuilder* builder, SkScalar distance,
                                      SkPathMeasure& meas) const {
#if defined(SK_BUILD_FOR_FUZZER)
    if (builder->countPoints() > 100000) {
        return fAdvance;
    }
#endif
    switch (fStyle) {
        case SkPath1DPathEffect::kTranslate_Style: {
            SkPoint pos;
            if (meas.getPosTan(distance, &pos, nullptr)) {
                builder->addPath(fPath, pos.fX, pos.fY);
            }
        } break;
        case SkPath1DPathEffect::kRotate_Style: {
            SkMatrix matrix;
            if (meas.getMatrix(distance, &matrix)) {
                builder->addPath(fPath, matrix);
            }
        } break;
        case SkPath1DPathEffect::kMorph_Style:
            morphpath(builder, fPath, meas, distance);
            break;
    }
    return fAdvance;
}

sk_sp<SkPathEffect> SkPath1DPathEffect::Make(const SkPath& path, SkScalar advance, SkScalar phase,
                                             Style style) {
    if (!skia_rs_path_1d_is_valid(advance, phase, path.isEmpty())) {
        return nullptr;
    }
    return sk_sp<SkPathEffect>(new SkPath1DPathEffectImpl(path, advance, phase, style));
}

void SkPath1DPathEffect::RegisterFlattenables() {
    SK_REGISTER_FLATTENABLE(SkPath1DPathEffectImpl);
}

// ============================================================================
// Sk2DPathEffect (src/effects/Sk2DPathEffect.cpp)
// ============================================================================

class Sk2DPathEffect : public SkPathEffectBase {
public:
    explicit Sk2DPathEffect(const SkMatrix& mat) : fMatrix(mat) {
        fMatrixIsInvertible = fMatrix.invert(&fInverse);
    }

protected:
    virtual void begin(const SkIRect&, SkPathBuilder*) const {}
    virtual void next(const SkPoint&, int, int, SkPathBuilder*) const {}
    virtual void end(SkPathBuilder*) const {}

    virtual void nextSpan(int x, int y, int ucount, SkPathBuilder* builder) const {
        if (!fMatrixIsInvertible) {
            return;
        }
#if defined(SK_BUILD_FOR_FUZZER)
        if (ucount > 100) {
            return;
        }
#endif
        const SkMatrix& mat = this->getMatrix();
        do {
            SkPoint dst = skia_rs_path_2d_cell_location(x, y, &mat);
            this->next(dst, x++, y, builder);
        } while (--ucount > 0);
    }

    const SkMatrix& getMatrix() const { return fMatrix; }

    void flatten(SkWriteBuffer& buffer) const override {
        buffer.writeMatrix(fMatrix);
    }

    bool onFilterPath(SkPathBuilder* dst, const SkPath& src, SkStrokeRec*,
                      const SkRect*, const SkMatrix&) const override {
        if (!fMatrixIsInvertible) {
            return false;
        }
        SkPath tmp = src.makeTransform(fInverse);
        SkIRect ir = tmp.getBounds().round();
        if (!ir.isEmpty()) {
            this->begin(ir, dst);
            SkRegion rgn;
            rgn.setPath(tmp, SkRegion(ir));
            SkRegion::Iterator iter(rgn);
            for (; !iter.done(); iter.next()) {
                const SkIRect& rect = iter.rect();
#if defined(SK_BUILD_FOR_FUZZER)
                if (rect.height() > 100) {
                    continue;
                }
#endif
                for (int y = rect.fTop; y < rect.fBottom; ++y) {
                    this->nextSpan(rect.fLeft, y, rect.width(), dst);
                }
            }
            this->end(dst);
        }
        return true;
    }

private:
    SkMatrix fMatrix, fInverse;
    bool fMatrixIsInvertible;
    bool computeFastBounds(SkRect*) const override { return false; }
};

class SkLine2DPathEffectImpl : public Sk2DPathEffect {
public:
    SkLine2DPathEffectImpl(SkScalar width, const SkMatrix& matrix)
        : Sk2DPathEffect(matrix)
        , fWidth(width) {
        SkASSERT(width >= 0);
    }

    bool onFilterPath(SkPathBuilder* dst, const SkPath& src, SkStrokeRec* rec,
                      const SkRect* cullRect, const SkMatrix& ctm) const override {
        if (this->INHERITED::onFilterPath(dst, src, rec, cullRect, ctm)) {
            rec->setStrokeStyle(fWidth);
            return true;
        }
        return false;
    }

    void nextSpan(int u, int v, int ucount, SkPathBuilder* dst) const override {
        SkPoint dstP[2];
        if (skia_rs_line_2d_span_endpoints(u, v, ucount, &this->getMatrix(), dstP)) {
            dst->moveTo(dstP[0]);
            dst->lineTo(dstP[1]);
        }
    }

    static sk_sp<SkFlattenable> CreateProc(SkReadBuffer& buffer) {
        SkMatrix matrix;
        buffer.readMatrix(&matrix);
        SkScalar width = buffer.readScalar();
        return SkLine2DPathEffect::Make(width, matrix);
    }

    void flatten(SkWriteBuffer& buffer) const override {
        buffer.writeMatrix(this->getMatrix());
        buffer.writeScalar(fWidth);
    }

    Factory getFactory() const override { return CreateProc; }
    const char* getTypeName() const override { return "SkLine2DPathEffect"; }

private:
    SkScalar fWidth;
    using INHERITED = Sk2DPathEffect;
};

class SkPath2DPathEffectImpl : public Sk2DPathEffect {
public:
    SkPath2DPathEffectImpl(const SkMatrix& m, const SkPath& p) : INHERITED(m), fPath(p) {}

    void next(const SkPoint& loc, int, int, SkPathBuilder* dst) const override {
        dst->addPath(fPath, loc.fX, loc.fY);
    }

    static sk_sp<SkFlattenable> CreateProc(SkReadBuffer& buffer) {
        SkMatrix matrix;
        buffer.readMatrix(&matrix);
        if (auto path = buffer.readPath()) {
            return SkPath2DPathEffect::Make(matrix, *path);
        }
        return nullptr;
    }

    void flatten(SkWriteBuffer& buffer) const override {
        buffer.writeMatrix(this->getMatrix());
        buffer.writePath(fPath);
    }

    Factory getFactory() const override { return CreateProc; }
    const char* getTypeName() const override { return "SkPath2DPathEffect"; }

private:
    SkPath fPath;
    using INHERITED = Sk2DPathEffect;
};

sk_sp<SkPathEffect> SkLine2DPathEffect::Make(SkScalar width, const SkMatrix& matrix) {
    if (!skia_rs_line_2d_is_valid(width)) {
        return nullptr;
    }
    return sk_sp<SkPathEffect>(new SkLine2DPathEffectImpl(width, matrix));
}

sk_sp<SkPathEffect> SkPath2DPathEffect::Make(const SkMatrix& matrix, const SkPath& path) {
    return sk_sp<SkPathEffect>(new SkPath2DPathEffectImpl(matrix, path));
}

void SkLine2DPathEffect::RegisterFlattenables() {
    SK_REGISTER_FLATTENABLE(SkLine2DPathEffectImpl);
}

void SkPath2DPathEffect::RegisterFlattenables() {
    SK_REGISTER_FLATTENABLE(SkPath2DPathEffectImpl);
}

// ============================================================================
// SkCornerPathEffect (src/effects/SkCornerPathEffect.cpp)
// ============================================================================

class SkCornerPathEffectImpl : public SkPathEffectBase {
public:
    explicit SkCornerPathEffectImpl(SkScalar radius) : fRadius(radius) {
        SkASSERT(radius > 0);
    }

    bool onFilterPath(SkPathBuilder* dst, const SkPath& src, SkStrokeRec*, const SkRect*,
                      const SkMatrix&) const override {
        if (fRadius <= 0) {
            return false;
        }
        RsCornerPathState* state = skia_rs_corner_state_new(fRadius);
        SkPath::Iter iter(src, false);
        RsCornerOutCmd cmds[3];
        while (auto rec = iter.next()) {
            const bool closed = (rec->fVerb == SkPathVerb::kMove) ? iter.isClosedContour() : false;
            const float w = (rec->fVerb == SkPathVerb::kConic) ? rec->conicWeight() : 0.0f;
            size_t n = skia_rs_corner_state_process(
                    state,
                    static_cast<uint8_t>(rec->fVerb),
                    rec->fPoints.data(),
                    rec->fPoints.size(),
                    w,
                    closed,
                    cmds);
            for (size_t i = 0; i < n; ++i) {
                switch (static_cast<SkPathVerb>(cmds[i].verb)) {
                    case SkPathVerb::kMove:
                        dst->moveTo(cmds[i].pts[0]);
                        break;
                    case SkPathVerb::kLine:
                        dst->lineTo(cmds[i].pts[0]);
                        break;
                    case SkPathVerb::kQuad:
                        dst->quadTo(cmds[i].pts[0], cmds[i].pts[1]);
                        break;
                    case SkPathVerb::kConic:
                        dst->conicTo(cmds[i].pts[0], cmds[i].pts[1], cmds[i].weight);
                        break;
                    case SkPathVerb::kCubic:
                        dst->cubicTo(cmds[i].pts[0], cmds[i].pts[1], cmds[i].pts[2]);
                        break;
                    case SkPathVerb::kClose:
                        dst->close();
                        break;
                }
            }
        }
        SkPoint lastCorner;
        if (skia_rs_corner_state_finish(state, &lastCorner)) {
            dst->lineTo(lastCorner);
        }
        skia_rs_corner_state_free(state);
        return true;
    }

    bool computeFastBounds(SkRect*) const override { return true; }

    static sk_sp<SkFlattenable> CreateProc(SkReadBuffer& buffer) {
        return SkCornerPathEffect::Make(buffer.readScalar());
    }

    void flatten(SkWriteBuffer& buffer) const override {
        buffer.writeScalar(fRadius);
    }

    Factory getFactory() const override { return CreateProc; }
    const char* getTypeName() const override { return "SkCornerPathEffect"; }

private:
    const SkScalar fRadius;
};

sk_sp<SkPathEffect> SkCornerPathEffect::Make(SkScalar radius) {
    return skia_rs_corner_is_valid(radius)
                   ? sk_sp<SkPathEffect>(new SkCornerPathEffectImpl(radius))
                   : nullptr;
}

void SkCornerPathEffect::RegisterFlattenables() {
    SkFlattenable::Register("SkCornerPathEffect", SkCornerPathEffectImpl::CreateProc);
}

// ============================================================================
// SkDashPathEffect (src/effects/SkDashPathEffect.cpp)
// ============================================================================

SkDashImpl::SkDashImpl(SkSpan<const SkScalar> intervals, SkScalar phase)
    : fIntervals(intervals.size())
    , fPhase(0)
    , fInitialDashLength(-1)
    , fIntervalLength(0)
    , fInitialDashIndex(0) {
    SkASSERT(intervals.size() > 1 && SkIsAlign2(intervals.size()));
    memcpy(fIntervals.data(), intervals.data(), intervals.size_bytes());
    SkDashPath::CalcDashParameters(phase, fIntervals,
                                   &fInitialDashLength, &fInitialDashIndex,
                                   &fIntervalLength, &fPhase);
}

bool SkDashImpl::onFilterPath(SkPathBuilder* builder, const SkPath& src, SkStrokeRec* rec,
                              const SkRect* cullRect, const SkMatrix&) const {
    return SkDashPath::InternalFilter(builder, src, rec, cullRect, fIntervals,
                                      fInitialDashLength, fInitialDashIndex, fIntervalLength,
                                      fPhase);
}

bool SkDashImpl::onAsPoints(PointData* results, const SkPath& src, const SkStrokeRec& rec,
                            const SkMatrix& matrix, const SkRect* cullRect) const {
    SkPoint pts[2];
    if (!src.isLine(pts)) {
        return false;
    }
    RsDashPointsResult* res = skia_rs_dash_as_points(
            fIntervals.data(),
            fIntervals.size(),
            fInitialDashLength,
            fInitialDashIndex,
            fIntervalLength,
            pts,
            rec.getWidth(),
            static_cast<int32_t>(rec.getCap()),
            SkPaint::kMiter_Join == rec.getJoin(),
            rec.getMiter(),
            &matrix,
            cullRect,
            results != nullptr);
    if (!res) {
        return false;
    }
    if (results) {
        const SkPoint* ptsPtr = nullptr;
        size_t ptsLen = 0;
        bool hasFirst = false;
        bool hasLast = false;
        SkRect firstRect = SkRect::MakeEmpty();
        SkRect lastRect = SkRect::MakeEmpty();
        skia_rs_dash_points_extract(res,
                                    &results->fSize,
                                    &results->fFlags,
                                    &ptsPtr,
                                    &ptsLen,
                                    &hasFirst,
                                    &firstRect,
                                    &hasLast,
                                    &lastRect);
        results->fNumPoints = static_cast<int>(ptsLen);
        results->fPoints.reset(results->fNumPoints);
        if (ptsLen > 0) {
            memcpy(results->fPoints.get(), ptsPtr, ptsLen * sizeof(SkPoint));
        }
        if (hasFirst) {
            results->fFirst = SkPath::Rect(firstRect);
        }
        if (hasLast) {
            results->fLast = SkPath::Rect(lastRect);
        }
    }
    skia_rs_dash_points_free(res);
    return true;
}

std::optional<SkPathEffectBase::DashInfo> SkDashImpl::asADash() const {
    return {{fIntervals, fPhase}};
}

void SkDashImpl::flatten(SkWriteBuffer& buffer) const {
    buffer.writeScalar(fPhase);
    buffer.writeScalarArray(fIntervals);
}

sk_sp<SkFlattenable> SkDashImpl::CreateProc(SkReadBuffer& buffer) {
    const SkScalar phase = buffer.readScalar();
    uint32_t count = buffer.getArrayCount();
    if (!buffer.validateCanReadN<SkScalar>(count)) {
        return nullptr;
    }
    AutoSTArray<32, SkScalar> intervals(count);
    if (buffer.readScalarArray(intervals)) {
        return SkDashPathEffect::Make(intervals, phase);
    }
    return nullptr;
}

sk_sp<SkPathEffect> SkDashPathEffect::Make(SkSpan<const SkScalar> intervals, SkScalar phase) {
    if (!SkDashPath::ValidDashPath(phase, intervals)) {
        return nullptr;
    }
    return sk_sp<SkPathEffect>(new SkDashImpl(intervals, phase));
}

// ============================================================================
// SkDiscretePathEffect (src/effects/SkDiscretePathEffect.cpp)
// ============================================================================

class SkDiscretePathEffectImpl : public SkPathEffectBase {
public:
    SkDiscretePathEffectImpl(SkScalar segLength, SkScalar deviation, uint32_t seedAssist)
        : fSegLength(segLength), fPerterb(deviation), fSeedAssist(seedAssist) {
        SkASSERT(skia_rs_discrete_is_valid(segLength, deviation));
    }

    bool onFilterPath(SkPathBuilder* dst, const SkPath& src, SkStrokeRec* rec,
                      const SkRect*, const SkMatrix&) const override {
        bool doFill = rec->isFillStyle();
        SkPathMeasure meas(src, doFill);
        uint32_t rngSeed = skia_rs_discrete_rng_init(fSeedAssist, meas.getLength());
        SkScalar scale = fPerterb;
        SkPoint p;
        SkVector v;

        do {
            SkScalar length = meas.getLength();
#if defined(SK_BUILD_FOR_FUZZER)
            if (length > 1000) {
                return false;
            }
#endif
            RsDiscreteContourPlan plan =
                    skia_rs_discrete_contour_plan(fSegLength, length, doFill, meas.isClosed());
            if (plan.copy_verbatim) {
                meas.getSegment(0, length, dst, true);
            } else {
                int n = plan.n;
                SkScalar delta = plan.delta;
                SkScalar distance = plan.initial_distance;
                if (meas.getPosTan(distance, &p, &v)) {
                    p = skia_rs_discrete_perturb(p, v, skia_rs_discrete_rng_next(&rngSeed) * scale);
                    dst->moveTo(p);
                }
                while (--n >= 0) {
                    distance += delta;
                    if (meas.getPosTan(distance, &p, &v)) {
                        p = skia_rs_discrete_perturb(
                                p, v, skia_rs_discrete_rng_next(&rngSeed) * scale);
                        dst->lineTo(p);
                    }
                }
                if (meas.isClosed()) {
                    dst->close();
                }
            }
        } while (meas.nextContour());
        return true;
    }

    bool computeFastBounds(SkRect* bounds) const override {
        if (bounds) {
            *bounds = skia_rs_discrete_compute_fast_bounds(*bounds, fPerterb);
        }
        return true;
    }

    static sk_sp<SkFlattenable> CreateProc(SkReadBuffer& buffer) {
        SkScalar segLength = buffer.readScalar();
        SkScalar perterb = buffer.readScalar();
        uint32_t seed = buffer.readUInt();
        return SkDiscretePathEffect::Make(segLength, perterb, seed);
    }

    void flatten(SkWriteBuffer& buffer) const override {
        buffer.writeScalar(fSegLength);
        buffer.writeScalar(fPerterb);
        buffer.writeUInt(fSeedAssist);
    }

    Factory getFactory() const override { return CreateProc; }
    const char* getTypeName() const override { return "SkDiscretePathEffect"; }

private:
    const SkScalar fSegLength, fPerterb;
    const uint32_t fSeedAssist;
};

sk_sp<SkPathEffect> SkDiscretePathEffect::Make(SkScalar segLength, SkScalar deviation,
                                               uint32_t seedAssist) {
    if (!skia_rs_discrete_is_valid(segLength, deviation)) {
        return nullptr;
    }
    return sk_sp<SkPathEffect>(new SkDiscretePathEffectImpl(segLength, deviation, seedAssist));
}

void SkDiscretePathEffect::RegisterFlattenables() {
    SkFlattenable::Register("SkDiscretePathEffect", SkDiscretePathEffectImpl::CreateProc);
}

// ============================================================================
// SkTrimPathEffect (src/effects/SkTrimPathEffect.cpp)
// ============================================================================

namespace {

static size_t add_segments(const SkPath& src, SkScalar start, SkScalar stop, SkPathBuilder* dst,
                           bool requires_moveto = true) {
    SkASSERT(start < stop);
    SkPathMeasure measure(src, false);
    SkScalar current_segment_offset = 0;
    size_t contour_count = 1;
    do {
        const auto next_offset = current_segment_offset + measure.getLength();
        if (start < next_offset) {
            measure.getSegment(start - current_segment_offset,
                               stop - current_segment_offset,
                               dst, requires_moveto);
            if (stop <= next_offset) {
                break;
            }
        }
        contour_count++;
        current_segment_offset = next_offset;
    } while (measure.nextContour());
    return contour_count;
}

}  // namespace

SkTrimPE::SkTrimPE(SkScalar startT, SkScalar stopT, SkTrimPathEffect::Mode mode)
    : fStartT(startT), fStopT(stopT), fMode(mode) {}

bool SkTrimPE::onFilterPath(SkPathBuilder* dst, const SkPath& src, SkStrokeRec*, const SkRect*,
                            const SkMatrix&) const {
    if (fStartT >= fStopT) {
        SkASSERT(fMode == SkTrimPathEffect::Mode::kNormal);
        return true;
    }

    SkScalar len = 0;
    SkPathMeasure meas(src, false);
    do {
        len += meas.getLength();
    } while (meas.nextContour());

    RsTrimSpanPlan plan = skia_rs_trim_compute_spans(
            len, fStartT, fStopT, static_cast<uint32_t>(fMode));
    if (!plan.is_inverted) {
        if (plan.has_first_span) {
            add_segments(src, plan.first_start, plan.first_stop, dst);
        }
    } else {
        bool requires_moveto = true;
        if (plan.has_first_span) {
            const auto contour_count = add_segments(src, plan.first_start, plan.first_stop, dst);
            if (contour_count == 1 && src.isLastContourClosed()) {
                requires_moveto = false;
            }
        }
        if (plan.has_second_span) {
            add_segments(src, plan.second_start, plan.second_stop, dst, requires_moveto);
        }
    }
    return true;
}

void SkTrimPE::flatten(SkWriteBuffer& buffer) const {
    buffer.writeScalar(fStartT);
    buffer.writeScalar(fStopT);
    buffer.writeUInt(static_cast<uint32_t>(fMode));
}

sk_sp<SkFlattenable> SkTrimPE::CreateProc(SkReadBuffer& buffer) {
    const auto start = buffer.readScalar(),
               stop = buffer.readScalar();
    const auto mode = buffer.readUInt();
    return SkTrimPathEffect::Make(
            start, stop,
            (mode & 1) ? SkTrimPathEffect::Mode::kInverted : SkTrimPathEffect::Mode::kNormal);
}

sk_sp<SkPathEffect> SkTrimPathEffect::Make(SkScalar startT, SkScalar stopT, Mode mode) {
    float pinnedStart = 0.0f;
    float pinnedStop = 0.0f;
    if (!skia_rs_trim_validate_and_pin(startT, stopT, static_cast<uint32_t>(mode),
                                       &pinnedStart, &pinnedStop)) {
        return nullptr;
    }
    return sk_sp<SkPathEffect>(new SkTrimPE(pinnedStart, pinnedStop, mode));
}
