/*
 * Safe Rust bridge for Layer 2.2 skia-rs-path (Core Path):
 *   SkPath.cpp, SkPathBuilder.cpp, SkPathData.cpp, SkPathDump.cpp,
 *   SkPathIter.cpp, SkPathRaw.cpp, SkPathRawShapes.cpp, SkPathPriv.cpp,
 *   SkPath_interpolate.cpp, SkPath_serial.cpp.
 */

#include "include/core/SkArc.h"
#include "include/core/SkData.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPath.h"
#include "include/core/SkPathBuilder.h"
#include "include/core/SkPathIter.h"
#include "include/core/SkPathTypes.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRRect.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkScalar.h"
#include "include/core/SkSpan.h"
#include "include/core/SkStream.h"
#include "include/core/SkString.h"
#include "include/core/SkTypes.h"
#include "include/private/SkAssert.h"
#include "include/private/SkDebug.h"
#include "include/private/SkFloatingPoint.h"
#include "include/private/SkMalloc.h"
#include "include/private/SkPathRef.h"
#include "include/private/SkSafe32.h"
#include "include/private/SkTo.h"
#include "src/core/SkGeometry.h"
#include "src/core/SkPathData.h"
#include "src/core/SkPathEnums.h"
#include "src/core/SkPathPriv.h"
#include "src/core/SkPathRaw.h"
#include "src/core/SkPathRawShapes.h"
#include "src/core/SkSafeMath.h"
#include "src/core/SkSpanPriv.h"

#include <algorithm>
#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>
#include <new>
#include <optional>
#include <utility>
#include <vector>

static_assert(sizeof(SkPoint) == 8);
static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkRRect) == 52);
static_assert(sizeof(SkMatrix) == 40);
static_assert(sizeof(SkPathVerb) == 1);

struct SkiaRsPathBuf;
struct SkiaRsString;

struct SkiaRsPathBufMeta {
    uint8_t fill_type;
    uint8_t convexity;
    uint8_t segment_mask;
    uint8_t is_a_type;
    uint8_t is_a_dir;
    uint8_t is_a_start_index;
    bool is_volatile;
    uint8_t _pad;
    int32_t last_move_index;
};

struct SkiaRsPathIterStep {
    bool has_rec;
    uint8_t verb;
    bool is_close_storage;
    uint8_t pt_len;
    size_t pt_offset;
    float conic_weight;
};

struct SkiaRsRRectShapeResult {
    uint8_t kind;
    uint8_t point_count;
    uint8_t convexity;
    uint8_t segment_mask;
};

struct SkiaRsRectContour {
    bool found;
    bool is_closed;
    uint8_t direction;
    uint8_t _pad;
    SkRect rect;
    size_t points_consumed;
    size_t verbs_consumed;
};

struct SkiaRsPathRectInfo {
    bool found;
    uint8_t direction;
    uint8_t start_index;
    uint8_t _pad;
    SkRect rect;
};

struct SkiaRsReadPathResult {
    uint8_t kind;
    uint8_t fill_type;
    uint8_t rrect_dir;
    uint8_t _pad;
    uint32_t rrect_start;
    size_t bytes_read;
    SkRRect rrect;
    SkiaRsPathBuf* general_buf;
};

extern "C" {
const SkPoint* skia_rs_path_buf_points(const SkiaRsPathBuf* buf, size_t* out_len);
const uint8_t* skia_rs_path_buf_verbs(const SkiaRsPathBuf* buf, size_t* out_len);
const float* skia_rs_path_buf_conics(const SkiaRsPathBuf* buf, size_t* out_len);
SkiaRsPathBufMeta skia_rs_path_buf_meta(const SkiaRsPathBuf* buf);
void skia_rs_path_buf_free(SkiaRsPathBuf* buf);

const uint8_t* skia_rs_string_data(const SkiaRsString* s, size_t* out_len);
void skia_rs_string_free(SkiaRsString* s);

bool skia_rs_path_valid_data(const SkPoint* pts, size_t pt_count,
                             const uint8_t* vbs, size_t vb_count,
                             const float* conics, size_t conic_count);
uint8_t skia_rs_path_compute_segment_mask(const uint8_t* vbs, size_t vb_count);
bool skia_rs_path_trimmed_bounds(const SkPoint* pts, size_t pt_count,
                                 const uint8_t* vbs, size_t vb_count,
                                 SkRect* out_rect);
int32_t skia_rs_path_find_last_move_to_index(const uint8_t* vbs, size_t vb_count, size_t pt_count);
bool skia_rs_path_is_axis_aligned(const SkPoint* pts, size_t pt_count);
bool skia_rs_path_is_line_degenerate(SkPoint p1, SkPoint p2, bool exact);
bool skia_rs_path_is_quad_degenerate(SkPoint p1, SkPoint p2, SkPoint p3, bool exact);
bool skia_rs_path_is_cubic_degenerate(SkPoint p1, SkPoint p2, SkPoint p3, SkPoint p4, bool exact);
bool skia_rs_path_is_zero_length_since_point(const SkPoint* pts, size_t pt_count, size_t start_idx);

SkiaRsPathIterStep skia_rs_path_iter_step(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count,
        size_t* p_index, size_t* v_index, size_t* c_index,
        SkPoint close_storage[2]);
bool skia_rs_contour_iter_next_counts(const uint8_t* vbs, size_t vb_count,
                                      size_t* out_npts, size_t* out_nvbs, size_t* out_nws);
bool skia_rs_legacy_iter_is_closed_contour(const uint8_t* vbs, size_t vb_count, bool force_close);

void skia_rs_shape_fill_rect_points(SkRect r, uint8_t dir, uint32_t index, SkPoint out_pts[4]);
void skia_rs_shape_fill_oval_points(SkRect r, uint8_t dir, uint32_t index, SkPoint out_pts[9]);
SkiaRsRRectShapeResult skia_rs_shape_build_rrect(
        const SkRRect* rrect, uint8_t dir, uint32_t index, SkPoint out_pts[13]);
uint8_t skia_rs_shape_tri_to_convexity(const SkPoint pts[3]);

SkiaRsRectContour skia_rs_path_is_rect_contour(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        uint32_t segment_mask, bool allow_partial);
bool skia_rs_path_is_nested_fill_rects(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count,
        SkRect bounds, uint8_t fill_type, uint8_t convexity, uint8_t segment_mask,
        SkRect out_rects[2], uint8_t out_dirs[2]);
SkiaRsPathRectInfo skia_rs_path_is_simple_rect(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count,
        uint32_t segment_mask, bool is_finite, bool is_simple_fill);
void skia_rs_path_transform_dir_and_start(
        const SkMatrix* matrix, bool is_rrect, uint8_t dir, uint32_t start,
        uint8_t* out_dir, uint32_t* out_start);
void skia_rs_path_deduce_rrect_from_contour(
        SkRect bounds, const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count, SkRRect* out_rrect);

uint8_t skia_rs_path_compute_convexity(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count);
uint8_t skia_rs_path_transform_convexity(
        const SkMatrix* matrix, const SkPoint* pts, size_t pt_count, uint8_t convexity);
uint8_t skia_rs_path_compute_first_direction(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count,
        SkRect bounds, uint8_t fill_type, uint8_t convexity, uint8_t segment_mask);
SkRect skia_rs_path_compute_tight_bounds(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count);
bool skia_rs_path_contains(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count,
        SkRect bounds, uint8_t fill_type, uint8_t convexity, uint8_t segment_mask,
        SkPoint p);
bool skia_rs_path_conservatively_contains_rect(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count,
        uint8_t convexity, bool is_finite, SkRect rect);
SkiaRsPathBuf* skia_rs_path_perspective_clip(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count,
        SkRect bounds, uint8_t fill_type, uint8_t convexity, uint8_t segment_mask,
        const SkMatrix* matrix);

bool skia_rs_path_draw_arc_is_convex(float sweep_angle, uint8_t arc_type, bool is_fill_no_pe);
SkiaRsPathBuf* skia_rs_path_create_draw_arc(
        SkRect oval, float start_angle, float sweep_angle, uint8_t arc_type, bool is_fill_no_pe);
bool skia_rs_path_can_interpolate(
        size_t from_pts_len, const uint8_t* from_vbs, size_t from_vb_len,
        const float* from_conics, size_t from_conic_len,
        size_t to_pts_len, const uint8_t* to_vbs, size_t to_vb_len,
        const float* to_conics, size_t to_conic_len);
void skia_rs_path_interpolate_points(
        const SkPoint* from_pts, const SkPoint* to_pts, size_t count, float t, SkPoint* out_pts);

SkiaRsPathBuf* skia_rs_builder_arc_to_oval(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count,
        uint8_t fill_type, uint8_t convexity, uint8_t segment_mask, int32_t last_move_index,
        uint8_t is_a_type, uint8_t is_a_dir, uint8_t is_a_start_index, bool is_volatile,
        SkRect oval, float start_angle, float sweep_angle, bool force_move_to);
SkiaRsPathBuf* skia_rs_builder_add_arc(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count,
        uint8_t fill_type, uint8_t convexity, uint8_t segment_mask, int32_t last_move_index,
        uint8_t is_a_type, uint8_t is_a_dir, uint8_t is_a_start_index, bool is_volatile,
        SkRect oval, float start_angle, float sweep_angle);
SkiaRsPathBuf* skia_rs_builder_arc_to_tangent(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count,
        uint8_t fill_type, uint8_t convexity, uint8_t segment_mask, int32_t last_move_index,
        uint8_t is_a_type, uint8_t is_a_dir, uint8_t is_a_start_index, bool is_volatile,
        SkPoint p1, SkPoint p2, float radius);
SkiaRsPathBuf* skia_rs_builder_arc_to_svg(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count,
        uint8_t fill_type, uint8_t convexity, uint8_t segment_mask, int32_t last_move_index,
        uint8_t is_a_type, uint8_t is_a_dir, uint8_t is_a_start_index, bool is_volatile,
        SkPoint rad, float angle, uint8_t arc_large, uint8_t sweep, SkPoint end_pt);
SkiaRsPathBuf* skia_rs_builder_transform(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count,
        uint8_t fill_type, uint8_t convexity, uint8_t segment_mask, int32_t last_move_index,
        uint8_t is_a_type, uint8_t is_a_dir, uint8_t is_a_start_index, bool is_volatile,
        const SkMatrix* matrix);
SkiaRsPathBuf* skia_rs_builder_reverse_path_to(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count,
        uint8_t fill_type, uint8_t convexity, uint8_t segment_mask, int32_t last_move_index,
        uint8_t is_a_type, uint8_t is_a_dir, uint8_t is_a_start_index, bool is_volatile,
        const SkPoint* src_pts, size_t src_pt_count,
        const uint8_t* src_vbs, size_t src_vb_count,
        const float* src_conics, size_t src_conic_count);
SkiaRsPathBuf* skia_rs_builder_reverse_add_path(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count,
        uint8_t fill_type, uint8_t convexity, uint8_t segment_mask, int32_t last_move_index,
        uint8_t is_a_type, uint8_t is_a_dir, uint8_t is_a_start_index, bool is_volatile,
        const SkPoint* src_pts, size_t src_pt_count,
        const uint8_t* src_vbs, size_t src_vb_count,
        const float* src_conics, size_t src_conic_count);
SkiaRsPathBuf* skia_rs_builder_add_path(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count,
        uint8_t fill_type, uint8_t convexity, uint8_t segment_mask, int32_t last_move_index,
        uint8_t is_a_type, uint8_t is_a_dir, uint8_t is_a_start_index, bool is_volatile,
        const SkPoint* src_pts, size_t src_pt_count,
        const uint8_t* src_vbs, size_t src_vb_count,
        const float* src_conics, size_t src_conic_count,
        uint8_t src_seg_mask, bool src_is_finite,
        uint8_t src_is_a_type, uint8_t src_is_a_dir, uint8_t src_is_a_start,
        uint8_t src_convexity, bool src_is_volatile,
        const SkMatrix* matrix, uint8_t mode);

size_t skia_rs_path_write_to_memory_as_rrect(
        uint8_t fill_type, bool has_rrect, const SkRRect* rrect,
        uint8_t direction, uint32_t start_index, uint8_t* dst, size_t dst_cap);
size_t skia_rs_path_write_to_memory(
        uint8_t fill_type,
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count,
        bool has_rrect, const SkRRect* rrect, uint8_t rrect_dir, uint32_t rrect_start,
        uint8_t* dst, size_t dst_cap);
SkiaRsReadPathResult skia_rs_path_read_from_memory(const uint8_t* src, size_t len);
SkiaRsString* skia_rs_path_dump_to_string(
        uint8_t fill_type,
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count,
        bool dump_as_hex, bool is_builder);
}  // extern "C"

namespace {

inline const uint8_t* verbs_u8(SkSpan<const SkPathVerb> vbs) {
    return reinterpret_cast<const uint8_t*>(vbs.data());
}

SkString take_rust_string(SkiaRsString* rs) {
    if (!rs) {
        return SkString();
    }
    size_t len = 0;
    const uint8_t* data = skia_rs_string_data(rs, &len);
    SkString out(reinterpret_cast<const char*>(data), len);
    skia_rs_string_free(rs);
    return out;
}

class SkSafeAccumulator {
public:
    explicit SkSafeAccumulator(size_t n = 0) : fTotal(n) {}
    bool ok() const { return fSafe.ok(); }
    SkSafeAccumulator& addMul(size_t a, size_t b) {
        fTotal = fSafe.add(fTotal, fSafe.mul(a, b));
        return *this;
    }
    std::optional<size_t> total() const {
        if (fSafe.ok()) {
            return fTotal;
        }
        return {};
    }
private:
    SkSafeMath fSafe;
    size_t fTotal;
};

uint64_t next_pathdata_unique_id() {
    constexpr int kHighBitsToMakeRoomForFillType = 2;
    constexpr uint64_t kMaxID =
            std::numeric_limits<uint64_t>::max() >> kHighBitsToMakeRoomForFillType;
    static std::atomic<uint64_t> nextID{1};
    uint64_t id = nextID.fetch_add(1, std::memory_order_relaxed);
    SkASSERT_RELEASE(id <= kMaxID);
    return id;
}

}  // namespace

// ============================================================================
// SkPathIter.cpp
// ============================================================================

std::optional<SkPathIter::Rec> SkPathIter::next() {
    auto step = skia_rs_path_iter_step(
            fPoints.data(), fPoints.size(),
            verbs_u8(fVerbs), fVerbs.size(),
            fConics.data(), fConics.size(),
            &pIndex, &vIndex, &cIndex,
            fClosePointStorage.data());
    if (!step.has_rec) {
        return {};
    }
    const SkPoint* base = step.is_close_storage
            ? (fClosePointStorage.data() + step.pt_offset)
            : (fPoints.data() + step.pt_offset);
    return Rec{{base, step.pt_len}, step.conic_weight, static_cast<SkPathVerb>(step.verb)};
}

std::optional<SkPathContourIter::Rec> SkPathContourIter::next() {
    size_t npts = 0, nvbs = 0, nws = 0;
    if (!skia_rs_contour_iter_next_counts(verbs_u8(fVerbs), fVerbs.size(), &npts, &nvbs, &nws)) {
        return {};
    }
    Rec rec = {
        fPoints.subspan(0, npts),
        fVerbs.subspan(0, nvbs),
        fConics.subspan(0, nws),
    };
    fPoints = fPoints.last(fPoints.size() - npts);
    fVerbs  = fVerbs.last(fVerbs.size() - nvbs);
    fConics = fConics.last(fConics.size() - nws);
    return rec;
}

// ============================================================================
// SkPathRaw.cpp
// ============================================================================

uint8_t SkPathPriv::ComputeSegmentMask(SkSpan<const SkPathVerb> verbs) {
    return skia_rs_path_compute_segment_mask(verbs_u8(verbs), verbs.size());
}

std::optional<SkRect> SkPathRaw::isRect() const {
    if (auto rc = SkPathPriv::IsRectContour(fPoints, fVerbs, fSegmentMask, false)) {
        return rc->fRect;
    }
    return {};
}

// ============================================================================
// SkPathRawShapes.cpp
// ============================================================================

static constexpr SkPathVerb gRectVerbs[] = {
    SkPathVerb::kMove,
    SkPathVerb::kLine,
    SkPathVerb::kLine,
    SkPathVerb::kLine,
    SkPathVerb::kClose,
};

static constexpr SkPathVerb gOvalVerbs[] = {
    SkPathVerb::kMove,
    SkPathVerb::kConic,
    SkPathVerb::kConic,
    SkPathVerb::kConic,
    SkPathVerb::kConic,
    SkPathVerb::kClose,
};

static constexpr float gFourQuarterCircleConics[] = {
    SK_ScalarRoot2Over2,
    SK_ScalarRoot2Over2,
    SK_ScalarRoot2Over2,
    SK_ScalarRoot2Over2,
};

static constexpr SkPathVerb gRRectVerbs_LineStart[] = {
    SkPathVerb::kMove,
    SkPathVerb::kLine, SkPathVerb::kConic,
    SkPathVerb::kLine, SkPathVerb::kConic,
    SkPathVerb::kLine, SkPathVerb::kConic,
    SkPathVerb::kLine, SkPathVerb::kConic,
    SkPathVerb::kClose,
};

static constexpr SkPathVerb gRRectVerbs_ConicStart[] = {
    SkPathVerb::kMove,
    SkPathVerb::kConic, SkPathVerb::kLine,
    SkPathVerb::kConic, SkPathVerb::kLine,
    SkPathVerb::kConic, SkPathVerb::kLine,
    SkPathVerb::kConic,
    SkPathVerb::kClose,
};

static constexpr SkPathVerb gTriangleVerbs[] = {
    SkPathVerb::kMove,
    SkPathVerb::kLine,
    SkPathVerb::kLine,
    SkPathVerb::kClose,
};

SkPathRawShapes::Rect::Rect(const SkRect& r, SkPathDirection dir, unsigned index) {
    skia_rs_shape_fill_rect_points(r, static_cast<uint8_t>(dir), index, fStorage);
    fPoints = {fStorage, 4};
    fVerbs = gRectVerbs;
    fConics = {};
    fBounds = r;
    fFillType = SkPathFillType::kWinding;
    fConvexity = SkPathDirection_ToConvexity(dir);
    fSegmentMask = kLine_SkPathSegmentMask;
}

SkPathRawShapes::Oval::Oval(const SkRect& r, SkPathDirection dir, unsigned index) {
    skia_rs_shape_fill_oval_points(r, static_cast<uint8_t>(dir), index, fStorage);
    fPoints = {fStorage, 9};
    fVerbs = gOvalVerbs;
    fConics = gFourQuarterCircleConics;
    fBounds = r;
    fFillType = SkPathFillType::kWinding;
    fConvexity = SkPathDirection_ToConvexity(dir);
    fSegmentMask = kConic_SkPathSegmentMask;
}

SkPathRawShapes::RRect::RRect(const SkRRect& rrect, SkPathDirection dir, unsigned index) {
    auto res = skia_rs_shape_build_rrect(&rrect, static_cast<uint8_t>(dir), index, fStorage);
    fPoints = {fStorage, res.point_count};
    switch (res.kind) {
        case 0:
            fVerbs = gRectVerbs;
            fConics = {};
            break;
        case 1:
            fVerbs = gOvalVerbs;
            fConics = gFourQuarterCircleConics;
            break;
        case 2:
            fVerbs = gRRectVerbs_LineStart;
            fConics = gFourQuarterCircleConics;
            break;
        default:
            fVerbs = gRRectVerbs_ConicStart;
            fConics = gFourQuarterCircleConics;
            break;
    }
    fBounds = rrect.getBounds();
    fFillType = SkPathFillType::kWinding;
    fConvexity = static_cast<SkPathConvexity>(res.convexity);
    fSegmentMask = res.segment_mask;
}

SkPathRawShapes::Triangle::Triangle(SkSpan<const SkPoint> threePoints, const SkRect& bounds)
    : SkPathRaw{threePoints,
                gTriangleVerbs,
                {},
                bounds,
                SkPathFillType::kDefault,
                static_cast<SkPathConvexity>(skia_rs_shape_tri_to_convexity(threePoints.data())),
                kLine_SkPathSegmentMask} {
    SkASSERT(threePoints.size() == 3);
}

// ============================================================================
// SkPathData.cpp
// ============================================================================

SkPathData* SkPathData::PeekEmptySingleton() {
    static SkPathData* gEmpty = SkPathData::MakeNoCheck({}, {}, {}, {}, {}).release();
    return gEmpty;
}

SkPathData::SkPathData(size_t npts, size_t nvbs, size_t ncns)
    : fUniqueID(next_pathdata_unique_id())
    , fConvexity((uint8_t)SkPathConvexity::kUnknown)
    , fType(SkPathIsAType::kGeneral) {
    if (nvbs != 0) {
        auto data = reinterpret_cast<std::byte*>(this + 1);
        fPoints = {reinterpret_cast<SkPoint*>(data), npts};
        data += fPoints.size_bytes();
        fConics = {reinterpret_cast<float*>(data), ncns};
        data += fConics.size_bytes();
        fVerbs = {reinterpret_cast<SkPathVerb*>(data), nvbs};
    }
}

SkPathData::~SkPathData() {
    SkDEBUGCODE(fUniqueID = 0xEEEEEEEEEEEEEEEEULL;)
}

void SkPathData::operator delete(void* p) {
    ::operator delete(p);
}

void SkPathData::addGenIDChangeListener(sk_sp<SkIDChangeListener> listener) const {
    if (this != SkPathData::PeekEmptySingleton()) {
        fGenIDChangeListeners.add(std::move(listener));
    }
}

sk_sp<SkPathData> SkPathData::Alloc(size_t npts, size_t nvbs, size_t ncns) {
    SkSafeAccumulator accum(sizeof(SkPathData));
    accum.addMul(npts, sizeof(SkPoint))
         .addMul(ncns, sizeof(float))
         .addMul(nvbs, sizeof(SkPathVerb));
    if (auto size = accum.total()) {
        void* storage = ::operator new(*size);
        return sk_sp<SkPathData>(new (storage) SkPathData(npts, nvbs, ncns));
    }
    return nullptr;
}

bool SkPathData::finishInit(std::optional<SkRect> bounds, std::optional<uint8_t> segmentMask) {
    if (fPoints.empty()) {
        fBounds = SkRect::MakeEmpty();
        fSegmentMask = 0;
        return true;
    }
    fSegmentMask = segmentMask.has_value()
            ? segmentMask.value()
            : SkPathPriv::ComputeSegmentMask(fVerbs);
    if (bounds.has_value()) {
        fBounds = bounds.value().makeSorted();
    } else {
        SkRect r;
        if (skia_rs_path_trimmed_bounds(
                    fPoints.data(), fPoints.size(), verbs_u8(fVerbs), fVerbs.size(), &r)) {
            fBounds = r;
        } else {
            return false;
        }
    }
    return true;
}

sk_sp<SkPathData> SkPathData::MakeTransform(const SkPathRaw& src, const SkMatrix& mx) {
    if (src.empty()) {
        return SkPathData::Empty();
    }
    if (mx.hasPerspective()) {
        return SkPathBuilder()
                .addRaw(src, SkPathBuilder::Reserve::kExact)
                .transform(mx)
                .detachData();
    }
    auto result = Alloc(src.points().size(), src.verbs().size(), src.conics().size());
    mx.mapPoints(result->fPoints, src.points());
    SkSpanPriv::Copy(result->fConics, src.conics());
    SkSpanPriv::Copy(result->fVerbs, src.verbs());

    std::optional<SkRect> transformedBounds;
    if (mx.rectStaysRect()) {
        transformedBounds = mx.mapRect(src.bounds());
        if (!transformedBounds.value().isFinite()) {
            return nullptr;
        }
    }
    if (!result->finishInit(transformedBounds, src.fSegmentMask)) {
        return nullptr;
    }
    result->setConvexity(SkPathPriv::TransformConvexity(mx, src.fPoints, src.fConvexity));
    return result;
}

sk_sp<SkPathData> SkPathData::makeTransform(const SkMatrix& mx) const {
    if (mx.isIdentity()) {
        return sk_ref_sp(this);
    }
    auto result = MakeTransform(this->raw(SkPathFillType::kDefault, SkResolveConvexity::kNo), mx);
    if (!result) {
        return nullptr;
    }
    if (fType == SkPathIsAType::kGeneral) {
        return result;
    }
    bool canMaintainIsA =
            mx.rectStaysRect() && SkPathPriv::IsAxisAligned(fPoints) && !result->bounds().isEmpty();
    if (!canMaintainIsA) {
        return result;
    }
    auto [dir, start] = SkPathPriv::TransformDirAndStart(
            mx, fType == SkPathIsAType::kRRect, fIsA.fDirection, fIsA.fStartIndex);
    result->setupIsA(fType, dir, start);
    return result;
}

sk_sp<SkPathData> SkPathData::makeOffset(SkVector v) const {
    return this->makeTransform(SkMatrix::Translate(v));
}

bool operator==(const SkPathData& a, const SkPathData& b) {
    if (&a == &b) {
        return true;
    }
    return SkSpanPriv::EQ(a.fPoints, b.fPoints) &&
           SkSpanPriv::EQ(a.fConics, b.fConics) &&
           SkSpanPriv::EQ(a.fVerbs, b.fVerbs);
}

sk_sp<SkPathData> SkPathData::MakeNoCheck(SkSpan<const SkPoint> pts,
                                          SkSpan<const SkPathVerb> vbs,
                                          SkSpan<const float> conics,
                                          std::optional<SkRect> bounds,
                                          std::optional<unsigned> segmentMask) {
    auto path = Alloc(pts.size(), vbs.size(), conics.size());
    SkSpanPriv::Copy(path->fPoints, pts);
    SkSpanPriv::Copy(path->fConics, conics);
    SkSpanPriv::Copy(path->fVerbs, vbs);
    std::optional<uint8_t> sm;
    if (segmentMask.has_value()) {
        sm = static_cast<uint8_t>(segmentMask.value());
    }
    return path->finishInit(bounds, sm) ? path : nullptr;
}

sk_sp<SkPathData> SkPathData::MakeNoCheck(const SkPathRaw& raw) {
    return MakeNoCheck(raw.points(), raw.verbs(), raw.conics(), raw.fBounds, raw.fSegmentMask);
}

sk_sp<SkPathData> SkPathData::Empty() {
    return sk_ref_sp(PeekEmptySingleton());
}

void SkPathData::setupIsA(SkPathIsAType type, SkPathDirection dir, unsigned index) {
    this->setConvexity(SkPathDirection_ToConvexity(dir));
    fType = type;
    fIsA.fDirection = dir;
    fIsA.fStartIndex = SkTo<uint8_t>(index);
}

sk_sp<SkPathData> SkPathData::Rect(const SkRect& r, SkPathDirection dir, unsigned index) {
    if (!r.isFinite()) {
        return nullptr;
    }
    SkPathRawShapes::Rect raw(r, dir, index);
    return MakeNoCheck(raw.points(), raw.verbs(), raw.conics(), raw.fBounds, raw.fSegmentMask);
}

sk_sp<SkPathData> SkPathData::Oval(const SkRect& r, SkPathDirection dir, unsigned index) {
    if (!r.isFinite()) {
        return nullptr;
    }
    SkPathRawShapes::Oval raw(r, dir, index);
    auto path = MakeNoCheck(raw.points(), raw.verbs(), raw.conics(), raw.fBounds, raw.fSegmentMask);
    path->setupIsA(SkPathIsAType::kOval, dir, index);
    return path;
}

sk_sp<SkPathData> SkPathData::RRect(const SkRRect& r, SkPathDirection dir, unsigned index) {
    if (!r.isValid()) {
        return nullptr;
    }
    SkPathRawShapes::RRect raw(r, dir, index);
    if (auto path = Make(raw.points(), raw.verbs(), raw.conics())) {
        path->setupIsA(SkPathIsAType::kRRect, dir, index);
        return path;
    }
    return nullptr;
}

sk_sp<SkPathData> SkPathData::Polygon(SkSpan<const SkPoint> pts, bool isClosed) {
    if (pts.size() == 0 || (pts.size() == 1 && !isClosed)) {
        return Empty();
    }
    const size_t nverbs = pts.size() + isClosed;
    auto path = Alloc(pts.size(), nverbs, 0);
    SkSpanPriv::Copy(path->fPoints, pts);
    path->fVerbs[0] = SkPathVerb::kMove;
    for (size_t i = 1; i < pts.size(); ++i) {
        path->fVerbs[i] = SkPathVerb::kLine;
    }
    if (isClosed) {
        path->fVerbs.back() = SkPathVerb::kClose;
    }
    return path->finishInit({}, kLine_SkPathSegmentMask) ? path : nullptr;
}

SkPathConvexity SkPathData::getConvexityOrUnknown() const {
    return static_cast<SkPathConvexity>(fConvexity.load(std::memory_order_relaxed));
}

SkPathConvexity SkPathData::getResolvedConvexity() const {
    auto convexity = this->getConvexityOrUnknown();
    if (convexity == SkPathConvexity::kUnknown) {
        convexity = SkPathPriv::ComputeConvexity(fPoints, fVerbs, fConics);
        this->setConvexity(convexity);
    }
    return convexity;
}

void SkPathData::setConvexity(SkPathConvexity convexity) const {
    fConvexity.store(static_cast<uint8_t>(convexity), std::memory_order_relaxed);
}

bool SkPathData::isConvex() const {
    return SkPathConvexity_IsConvex(this->getResolvedConvexity());
}

SkRect SkPathData::computeTightBounds() const {
    return SkPathPriv::ComputeTightBounds(this->points(), this->verbs(), this->conics());
}

SkPathRaw SkPathData::raw(SkPathFillType ft, SkResolveConvexity rc) const {
    return {
        fPoints,
        fVerbs,
        fConics,
        fBounds,
        ft,
        rc == SkResolveConvexity::kYes ? this->getResolvedConvexity()
                                       : this->getConvexityOrUnknown(),
        fSegmentMask,
    };
}

std::optional<std::array<SkPoint, 2>> SkPathData::asLine() const {
    if (fPoints.size() == 2 && fVerbs.size() == 2 && fConics.size() == 0 &&
        fVerbs[1] == SkPathVerb::kLine) {
        return {{fPoints[0], fPoints[1]}};
    }
    return {};
}

std::optional<SkPathRectInfo> SkPathData::asRect() const {
    if (auto rc = SkPathPriv::IsRectContour(fPoints, fVerbs, fSegmentMask, false)) {
        return {{fBounds, rc->fDirection, 0}};
    }
    return {};
}

std::optional<SkPathOvalInfo> SkPathData::asOval() const {
    if (fType == SkPathIsAType::kOval) {
        return {{fBounds, fIsA.fDirection, fIsA.fStartIndex}};
    }
    return {};
}

std::optional<SkPathRRectInfo> SkPathData::asRRect() const {
    if (fType == SkPathIsAType::kRRect) {
        return {{
            SkPathPriv::DeduceRRectFromContour(fBounds, fPoints, fVerbs),
            fIsA.fDirection,
            fIsA.fStartIndex,
        }};
    }
    return {};
}

bool SkPathData::contains(SkPoint p, SkPathFillType ft) const {
    return SkPathPriv::Contains(this->raw(ft, SkResolveConvexity::kNo), p);
}

sk_sp<SkPathData> SkPathData::Make(SkSpan<const SkPoint> pts,
                                   SkSpan<const SkPathVerb> vbs,
                                   SkSpan<const float> conics) {
    if (!skia_rs_path_valid_data(
                pts.data(), pts.size(), verbs_u8(vbs), vbs.size(), conics.data(), conics.size())) {
        return nullptr;
    }
    return MakeNoCheck(pts, vbs, conics, {}, {});
}

// ============================================================================
// SkPathPriv.cpp
// ============================================================================

int SkPathPriv::GenIDChangeListenersCount(const SkPath& path) {
    return path.fPathData->genIDChangeListenerCount();
}

void SkPathPriv::AddGenIDChangeListener(const SkPath& path, sk_sp<SkIDChangeListener> listener) {
    path.fPathData->addGenIDChangeListener(std::move(listener));
}

std::optional<SkPathPriv::RectContour> SkPathPriv::IsRectContour(
        SkSpan<const SkPoint> ptSpan,
        SkSpan<const SkPathVerb> verbSpan,
        uint32_t segmentMask,
        bool allowPartial) {
    auto res = skia_rs_path_is_rect_contour(
            ptSpan.data(), ptSpan.size(),
            verbs_u8(verbSpan), verbSpan.size(),
            segmentMask, allowPartial);
    if (!res.found) {
        return {};
    }
    return RectContour{
        res.rect,
        res.is_closed,
        static_cast<SkPathDirection>(res.direction),
        res.points_consumed,
        res.verbs_consumed,
    };
}

bool SkPathPriv::IsNestedFillRects(
        const SkPathRaw& raw, SkRect rects[2], SkPathDirection dirs[2]) {
    uint8_t rawDirs[2] = {0, 0};
    bool ok = skia_rs_path_is_nested_fill_rects(
            raw.fPoints.data(), raw.fPoints.size(),
            verbs_u8(raw.fVerbs), raw.fVerbs.size(),
            raw.fConics.data(), raw.fConics.size(),
            raw.fBounds,
            static_cast<uint8_t>(raw.fFillType),
            static_cast<uint8_t>(raw.fConvexity),
            raw.fSegmentMask,
            rects,
            dirs ? rawDirs : nullptr);
    if (ok && dirs) {
        dirs[0] = static_cast<SkPathDirection>(rawDirs[0]);
        dirs[1] = static_cast<SkPathDirection>(rawDirs[1]);
    }
    return ok;
}

bool SkPathPriv::DrawArcIsConvex(
        SkScalar sweepAngle, SkArc::Type arcType, bool isFillNoPathEffect) {
    return skia_rs_path_draw_arc_is_convex(
            sweepAngle, static_cast<uint8_t>(arcType), isFillNoPathEffect);
}

static SkPath path_from_rust_buf(SkiaRsPathBuf* buf) {
    if (!buf) {
        return SkPath();
    }
    size_t npts = 0, nvbs = 0, ncns = 0;
    const SkPoint* pts = skia_rs_path_buf_points(buf, &npts);
    const uint8_t* vbs = skia_rs_path_buf_verbs(buf, &nvbs);
    const float* cns = skia_rs_path_buf_conics(buf, &ncns);
    auto meta = skia_rs_path_buf_meta(buf);
    SkPath path = SkPath::Raw(
            {pts, npts},
            {reinterpret_cast<const SkPathVerb*>(vbs), nvbs},
            {cns, ncns},
            static_cast<SkPathFillType>(meta.fill_type),
            meta.is_volatile);
    if (meta.convexity != static_cast<uint8_t>(SkPathConvexity::kUnknown)) {
        SkPathPriv::SetConvexity(path, static_cast<SkPathConvexity>(meta.convexity));
    }
    skia_rs_path_buf_free(buf);
    return path;
}

SkPath SkPathPriv::CreateDrawArcPath(const SkArc& arc, bool isFillNoPathEffect) {
    auto* buf = skia_rs_path_create_draw_arc(
            arc.oval(), arc.startAngle(), arc.sweepAngle(),
            arc.isWedge() ? 1 : 0, isFillNoPathEffect);
    return path_from_rust_buf(buf);
}

SkPathConvexity SkPathPriv::ComputeConvexity(
        SkSpan<const SkPoint> points,
        SkSpan<const SkPathVerb> verbs,
        SkSpan<const float> conicWeights) {
    return static_cast<SkPathConvexity>(skia_rs_path_compute_convexity(
            points.data(), points.size(),
            verbs_u8(verbs), verbs.size(),
            conicWeights.data(), conicWeights.size()));
}

SkPathFirstDirection SkPathPriv::ComputeFirstDirection(const SkPathRaw& raw) {
    return static_cast<SkPathFirstDirection>(skia_rs_path_compute_first_direction(
            raw.fPoints.data(), raw.fPoints.size(),
            verbs_u8(raw.fVerbs), raw.fVerbs.size(),
            raw.fConics.data(), raw.fConics.size(),
            raw.fBounds,
            static_cast<uint8_t>(raw.fFillType),
            static_cast<uint8_t>(raw.fConvexity),
            raw.fSegmentMask));
}

bool SkPathPriv::Contains(const SkPathRaw& raw, SkPoint p) {
    return skia_rs_path_contains(
            raw.fPoints.data(), raw.fPoints.size(),
            verbs_u8(raw.fVerbs), raw.fVerbs.size(),
            raw.fConics.data(), raw.fConics.size(),
            raw.fBounds,
            static_cast<uint8_t>(raw.fFillType),
            static_cast<uint8_t>(raw.fConvexity),
            raw.fSegmentMask,
            p);
}

bool SkPathPriv::IsAxisAligned(SkSpan<const SkPoint> pts) {
    return skia_rs_path_is_axis_aligned(pts.data(), pts.size());
}

SkPathConvexity SkPathPriv::TransformConvexity(
        const SkMatrix& matrix, SkSpan<const SkPoint> pts, SkPathConvexity convexity) {
    return static_cast<SkPathConvexity>(skia_rs_path_transform_convexity(
            &matrix, pts.data(), pts.size(), static_cast<uint8_t>(convexity)));
}

SkRect SkPathPriv::ComputeTightBounds(
        SkSpan<const SkPoint> points,
        SkSpan<const SkPathVerb> verbs,
        SkSpan<const float> conicWeights) {
    return skia_rs_path_compute_tight_bounds(
            points.data(), points.size(),
            verbs_u8(verbs), verbs.size(),
            conicWeights.data(), conicWeights.size());
}

int SkPathPriv::FindLastMoveToIndex(SkSpan<const SkPathVerb> verbs, const size_t ptCount) {
    return skia_rs_path_find_last_move_to_index(verbs_u8(verbs), verbs.size(), ptCount);
}

std::pair<SkPathDirection, unsigned> SkPathPriv::TransformDirAndStart(
        const SkMatrix& matrix, bool isRRect, SkPathDirection dir, unsigned start) {
    uint8_t outDir = 0;
    uint32_t outStart = 0;
    skia_rs_path_transform_dir_and_start(
            &matrix, isRRect, static_cast<uint8_t>(dir), start, &outDir, &outStart);
    return {static_cast<SkPathDirection>(outDir), outStart};
}

SkRRect SkPathPriv::DeduceRRectFromContour(
        const SkRect& bounds, SkSpan<const SkPoint> pts, SkSpan<const SkPathVerb> vbs) {
    SkRRect out;
    skia_rs_path_deduce_rrect_from_contour(
            bounds, pts.data(), pts.size(), verbs_u8(vbs), vbs.size(), &out);
    return out;
}

// ============================================================================
// SkPath.cpp
// ============================================================================

SkPath::SkPath(sk_sp<SkPathData> pd, SkPathFillType ft, bool isVolatile)
    : fPathData(std::move(pd))
    , fFillType(ft)
    , fIsVolatile(isVolatile) {
    SkASSERT(fPathData);
}

SkPath::SkPath(SkPathFillType ft)
    : fPathData(SkPathData::Empty())
    , fFillType(ft)
    , fIsVolatile(false) {}

SkPath::SkPath(const SkPath&) = default;
SkPath& SkPath::operator=(const SkPath&) = default;
SkPath::SkPath(SkPath&&) = default;
SkPath& SkPath::operator=(SkPath&&) = default;

SkPath::~SkPath() {
    SkDEBUGCODE(this->validate();)
}

void SkPath::setConvexity(SkPathConvexity c) const { fPathData->setConvexity(c); }
SkPathConvexity SkPath::getConvexityOrUnknown() const { return fPathData->getConvexityOrUnknown(); }

bool operator==(const SkPath& a, const SkPath& b) {
    return &a == &b || (a.fFillType == b.fFillType && *a.fPathData == *b.fPathData);
}

void SkPath::swap(SkPath& that) {
    if (this != &that) {
        fPathData.swap(that.fPathData);
        std::swap(fFillType, that.fFillType);
        std::swap(fIsVolatile, that.fIsVolatile);
    }
}

SkPath& SkPath::reset() {
    *this = SkPath();
    return *this;
}

const SkRect& SkPath::getBounds() const { return fPathData->bounds(); }
uint32_t SkPath::getSegmentMasks() const { return fPathData->segmentMask(); }
bool SkPath::isFinite() const { return fPathData.get() != PeekErrorSingleton(); }
bool SkPath::isValid() const { return this->isFinite(); }
uint64_t SkPath::getGenerationID() const { return fPathData->uniqueID(); }

#ifdef SK_DEBUG
void SkPath::validate() const {}
#endif

std::optional<SkPathOvalInfo> SkPath::getOvalInfo() const { return fPathData->asOval(); }
std::optional<SkPathRRectInfo> SkPath::getRRectInfo() const { return fPathData->asRRect(); }

SkSpan<const SkPoint> SkPath::points() const { return fPathData->points(); }
SkSpan<const SkPathVerb> SkPath::verbs() const { return fPathData->verbs(); }
SkSpan<const float> SkPath::conicWeights() const { return fPathData->conics(); }

SkPath SkPath::Raw(SkSpan<const SkPoint> pts, SkSpan<const SkPathVerb> vbs,
                   SkSpan<const float> ws, SkPathFillType ft, bool isVolatile) {
    return MakeNullCheck(SkPathData::Make(pts, vbs, ws), ft, isVolatile);
}

SkPath SkPath::Rect(const SkRect& r, SkPathFillType ft, SkPathDirection dir, unsigned startIndex) {
    startIndex &= 3;
    return MakeNullCheck(SkPathData::Rect(r, dir, startIndex), ft, false);
}

SkPath SkPath::Oval(const SkRect& r, SkPathDirection dir, unsigned startIndex) {
    startIndex &= 3;
    return MakeNullCheck(SkPathData::Oval(r, dir, startIndex), SkPathFillType::kDefault, false);
}

SkPath SkPath::RRect(const SkRRect& rr, SkPathDirection dir, unsigned startIndex) {
    startIndex &= 7;
    const SkRect& bounds = rr.getBounds();
    auto [asType, newIndex] = SkPathPriv::SimplifyRRect(rr, startIndex);
    switch (asType) {
        case SkPathPriv::RRectAsEnum::kRect:
            return SkPath::Rect(bounds, SkPathFillType::kDefault, dir, newIndex);
        case SkPathPriv::RRectAsEnum::kOval:
            return SkPath::Oval(bounds, dir, newIndex);
        case SkPathPriv::RRectAsEnum::kRRect:
            break;
    }
    return MakeNullCheck(SkPathData::RRect(rr, dir, newIndex), SkPathFillType::kDefault, false);
}

SkPath SkPath::Polygon(SkSpan<const SkPoint> pts, bool isClosed,
                       SkPathFillType ft, bool isVolatile) {
    return MakeNullCheck(SkPathData::Polygon(pts, isClosed), ft, isVolatile);
}

std::optional<SkPath> SkPath::tryMakeTransform(const SkMatrix& matrix) const {
    if (auto pdata = fPathData->makeTransform(matrix)) {
        return SkPath(std::move(pdata), fFillType, fIsVolatile);
    }
    return {};
}

SkPath SkPath::makeTransform(const SkMatrix& matrix) const {
    if (!this->isFinite()) {
        return *this;
    }
    if (auto newpath = this->tryMakeTransform(matrix)) {
        return *newpath;
    }
    return SkPath(sk_ref_sp(PeekErrorSingleton()), fFillType, false);
}

std::optional<SkPathRaw> SkPath::raw(SkResolveConvexity rc) const {
    return fPathData->raw(fFillType, rc);
}

bool SkPath::conservativelyContainsRect(const SkRect& rect) const {
    const SkPathConvexity convexity = this->getConvexity();
    auto pts = this->points();
    auto vbs = this->verbs();
    auto cns = this->conicWeights();
    return skia_rs_path_conservatively_contains_rect(
            pts.data(), pts.size(),
            verbs_u8(vbs), vbs.size(),
            cns.data(), cns.size(),
            static_cast<uint8_t>(convexity),
            this->isFinite(),
            rect);
}

bool SkPath::isLastContourClosed() const {
    auto vbs = this->verbs();
    return !vbs.empty() && vbs.back() == SkPathVerb::kClose;
}

bool SkPath::isLine(SkPoint line[2]) const {
    auto vbs = this->verbs();
    if (vbs.size() == 2 && vbs[1] == SkPathVerb::kLine) {
        auto pts = this->points();
        if (line) {
            line[0] = pts[0];
            line[1] = pts[1];
        }
        return true;
    }
    return false;
}

bool SkPath::isEmpty() const {
    SkDEBUGCODE(this->validate();)
    return this->verbs().empty();
}

bool SkPath::isConvex() const {
    return SkPathConvexity_IsConvex(this->getConvexity());
}

bool SkPath::isRect(SkRect* rect, bool* isClosed, SkPathDirection* direction) const {
    SkDEBUGCODE(this->validate();)
    if (auto rc = SkPathPriv::IsRectContour(this->points(), this->verbs(),
                                            this->getSegmentMasks(), false)) {
        if (rect) {
            *rect = rc->fRect;
        }
        if (isClosed) {
            *isClosed = rc->fIsClosed;
        }
        if (direction) {
            *direction = rc->fDirection;
        }
        return true;
    }
    return false;
}

bool SkPath::isOval(SkRect* bounds) const {
    if (auto info = this->getOvalInfo()) {
        if (bounds) {
            *bounds = info->fBounds;
        }
        return true;
    }
    return false;
}

bool SkPath::isRRect(SkRRect* rrect) const {
    if (auto info = this->getRRectInfo()) {
        if (rrect) {
            *rrect = info->fRRect;
        }
        return true;
    }
    return false;
}

#ifdef SK_LEGACY_PATH_ACCESSORS
size_t SkPath::getPoints(SkSpan<SkPoint> dst) const {
    SkDEBUGCODE(this->validate();)
    auto src = this->points();
    const size_t n = std::min(dst.size(), src.size());
    sk_careful_memcpy(dst.data(), src.data(), n * sizeof(SkPoint));
    return src.size();
}

SkPoint SkPath::getPoint(int index) const {
    auto pts = this->points();
    if ((unsigned)index < (unsigned)pts.size()) {
        return pts[index];
    }
    return SkPoint::Make(0, 0);
}

size_t SkPath::getVerbs(SkSpan<uint8_t> dst) const {
    SkDEBUGCODE(this->validate();)
    auto src = this->verbs();
    const size_t n = std::min(dst.size(), src.size());
    sk_careful_memcpy(dst.data(), src.data(), n);
    return src.size();
}
#endif

size_t SkPath::approximateBytesUsed() const {
    return sizeof(SkPath)
         + this->points().size_bytes()
         + this->verbs().size_bytes()
         + this->conicWeights().size_bytes();
}

std::optional<SkPoint> SkPath::getLastPt() const {
    SkDEBUGCODE(this->validate();)
    auto pts = this->points();
    if (!pts.empty()) {
        return pts.back();
    }
    return {};
}

SkPathConvexity SkPath::getConvexity() const {
    SkPathConvexity convexity = this->getConvexityOrUnknown();
    if (convexity == SkPathConvexity::kUnknown) {
        convexity = this->computeConvexity();
    }
    return convexity;
}

SkPathIter SkPath::iter() const {
    return {this->points(), this->verbs(), this->conicWeights()};
}

SkPath::Iter::Iter() {
#ifdef SK_DEBUG
    fPts = nullptr;
    fConicWeights = nullptr;
    fMoveTo.fX = fMoveTo.fY = fLastPt.fX = fLastPt.fY = 0;
    fForceClose = fCloseLine = false;
#endif
    fVerbs = nullptr;
    fVerbStop = nullptr;
    fNeedClose = false;
}

SkPath::Iter::Iter(const SkPath& path, bool forceClose) {
    this->setPath(path, forceClose);
}

void SkPath::Iter::setPath(const SkPath& path, bool forceClose) {
    fPts = path.points().data();
    const SkSpan<const SkPathVerb> vbs = path.verbs();
    fVerbs = vbs.begin();
    fVerbStop = vbs.end();
    fConicWeights = path.conicWeights().data();
    if (fConicWeights) {
        fConicWeights -= 1;
    }
    fLastPt.fX = fLastPt.fY = 0;
    fMoveTo.fX = fMoveTo.fY = 0;
    fForceClose = SkToU8(forceClose);
    fNeedClose = false;
}

bool SkPath::Iter::isClosedContour() const {
    if (fVerbs == nullptr || fVerbs == fVerbStop) {
        return false;
    }
    return skia_rs_legacy_iter_is_closed_contour(
            reinterpret_cast<const uint8_t*>(fVerbs),
            static_cast<size_t>(fVerbStop - fVerbs),
            SkToBool(fForceClose));
}

SkPathVerb SkPath::Iter::autoClose(SkPoint pts[2]) {
    SkASSERT(pts);
    if (fLastPt != fMoveTo) {
        if (SkIsNaN(fLastPt.fX) || SkIsNaN(fLastPt.fY) ||
            SkIsNaN(fMoveTo.fX) || SkIsNaN(fMoveTo.fY)) {
            return SkPathVerb::kClose;
        }
        pts[0] = fLastPt;
        pts[1] = fMoveTo;
        fLastPt = fMoveTo;
        fCloseLine = true;
        return SkPathVerb::kLine;
    }
    pts[0] = fMoveTo;
    return SkPathVerb::kClose;
}

SkPath::Verb SkPath::Iter::next(SkPoint ptsParam[4]) {
    SkASSERT(ptsParam);
    if (fVerbs == fVerbStop) {
        if (fNeedClose) {
            if (SkPathVerb::kLine == this->autoClose(ptsParam)) {
                return kLine_Verb;
            }
            fNeedClose = false;
            return kClose_Verb;
        }
        return kDone_Verb;
    }

    SkPathVerb verb = *fVerbs++;
    const SkPoint* srcPts = fPts;
    SkPoint* pts = ptsParam;

    switch (verb) {
        case SkPathVerb::kMove:
            if (fNeedClose) {
                fVerbs--;
                verb = this->autoClose(pts);
                if (verb == SkPathVerb::kClose) {
                    fNeedClose = false;
                }
                return (Verb)verb;
            }
            if (fVerbs == fVerbStop) {
                return kDone_Verb;
            }
            fMoveTo = *srcPts;
            pts[0] = *srcPts;
            srcPts += 1;
            fLastPt = fMoveTo;
            fNeedClose = fForceClose;
            break;
        case SkPathVerb::kLine:
            pts[0] = fLastPt;
            pts[1] = srcPts[0];
            fLastPt = srcPts[0];
            fCloseLine = false;
            srcPts += 1;
            break;
        case SkPathVerb::kConic:
            fConicWeights += 1;
            [[fallthrough]];
        case SkPathVerb::kQuad:
            pts[0] = fLastPt;
            memcpy(&pts[1], srcPts, 2 * sizeof(SkPoint));
            fLastPt = srcPts[1];
            srcPts += 2;
            break;
        case SkPathVerb::kCubic:
            pts[0] = fLastPt;
            memcpy(&pts[1], srcPts, 3 * sizeof(SkPoint));
            fLastPt = srcPts[2];
            srcPts += 3;
            break;
        case SkPathVerb::kClose:
            verb = this->autoClose(pts);
            if (verb == SkPathVerb::kLine) {
                fVerbs--;
            } else {
                fNeedClose = false;
            }
            fLastPt = fMoveTo;
            break;
    }
    fPts = srcPts;
    return (Verb)verb;
}

static inline uint8_t SkPathIterPointsPerVerb(SkPathVerb verb) {
    static constexpr auto gCounts = std::to_array<uint8_t>({1, 2, 3, 3, 4, 0});
    return gCounts[static_cast<unsigned>(verb)];
}

std::optional<SkPath::IterRec> SkPath::Iter::next() {
    auto legacyVerb = this->next(fStorage.data());
    if (legacyVerb == kDone_Verb) {
        return {};
    }
    SkPathVerb verb = static_cast<SkPathVerb>(legacyVerb);
    return {{
        verb,
        {fStorage.data(), SkPathIterPointsPerVerb(verb)},
        verb == SkPathVerb::kConic ? *fConicWeights : 1,
    }};
}

void SkPath::RawIter::setPath(const SkPath& path) {
    SkPathPriv::Iterate iterate(path);
    fIter = iterate.begin();
    fEnd = iterate.end();
}

SkPath::Verb SkPath::RawIter::next(SkPoint pts[4]) {
    if (!(fIter != fEnd)) {
        return kDone_Verb;
    }
    auto [verb, iterPts, weights] = *fIter;
    int numPts = 0;
    switch (verb) {
        case SkPathVerb::kMove:  numPts = 1; break;
        case SkPathVerb::kLine:  numPts = 2; break;
        case SkPathVerb::kQuad:  numPts = 3; break;
        case SkPathVerb::kConic: numPts = 3; fConicWeight = *weights; break;
        case SkPathVerb::kCubic: numPts = 4; break;
        case SkPathVerb::kClose: numPts = 0; break;
    }
    memcpy(pts, iterPts, sizeof(SkPoint) * numPts);
    ++fIter;
    return (Verb)verb;
}

std::optional<SkPath::IterRec> SkPath::RawIter::next() {
    if (fIter == fEnd) {
        return {};
    }
    auto [verb, iterPts, weights] = *fIter++;
    return {{
        verb,
        {iterPts, SkPathIterPointsPerVerb(verb)},
        verb == SkPathVerb::kConic ? *weights : 1,
    }};
}

SkPath SkPath::makeFillType(SkPathFillType ft) const {
    SkPath copy = *this;
    copy.setFillType(ft);
    return copy;
}

SkPath SkPath::makeToggleInverseFillType() const {
    return this->makeFillType(SkPathFillType_ToggleInverse(fFillType));
}

SkPath SkPath::makeIsVolatile(bool v) const {
    SkPath copy = *this;
    copy.fIsVolatile = v;
    return copy;
}

SkPathConvexity SkPath::computeConvexity() const {
    if (auto c = this->getConvexityOrUnknown(); c != SkPathConvexity::kUnknown) {
        return c;
    }
    SkPathConvexity convexity = SkPathConvexity::kConcave;
    if (this->isFinite()) {
        convexity = SkPathPriv::ComputeConvexity(
                this->points(), this->verbs(), this->conicWeights());
    }
    this->setConvexity(convexity);
    return convexity;
}

bool SkPath::contains(SkPoint p) const {
    const auto raw = SkPathPriv::Raw(*this, SkResolveConvexity::kNo);
    return raw.has_value() && SkPathPriv::Contains(*raw, p);
}

int SkPath::ConvertConicToQuads(const SkPoint& p0, const SkPoint& p1, const SkPoint& p2,
                                SkScalar w, SkPoint pts[], int pow2) {
    const SkConic conic(p0, p1, p2, w);
    return conic.chopIntoQuadsPOW2(pts, pow2);
}

SkRect SkPath::computeTightBounds() const {
    if (this->getSegmentMasks() == SkPath::kLine_SegmentMask) {
        return this->getBounds();
    }
    return SkPathPriv::ComputeTightBounds(this->points(), this->verbs(), this->conicWeights());
}

bool SkPath::IsLineDegenerate(const SkPoint& p1, const SkPoint& p2, bool exact) {
    return skia_rs_path_is_line_degenerate(p1, p2, exact);
}

bool SkPath::IsQuadDegenerate(const SkPoint& p1, const SkPoint& p2, const SkPoint& p3, bool exact) {
    return skia_rs_path_is_quad_degenerate(p1, p2, p3, exact);
}

bool SkPath::IsCubicDegenerate(const SkPoint& p1, const SkPoint& p2,
                               const SkPoint& p3, const SkPoint& p4, bool exact) {
    return skia_rs_path_is_cubic_degenerate(p1, p2, p3, p4, exact);
}

SkPath SkPath::RRect(const SkRRect& rr, SkPathDirection dir) {
    return RRect(rr, dir, dir == SkPathDirection::kCW ? 6 : 7);
}

SkPath SkPath::Oval(const SkRect& r, SkPathDirection dir) {
    return Oval(r, dir, 1);
}

SkPath SkPath::Circle(SkScalar x, SkScalar y, SkScalar r, SkPathDirection dir) {
    if (r >= 0) {
        return Oval(SkRect::MakeLTRB(x - r, y - r, x + r, y + r), dir);
    }
    return SkPath();
}

SkPath SkPath::RRect(const SkRect& r, SkScalar rx, SkScalar ry, SkPathDirection dir) {
    return RRect(SkRRect::MakeRectXY(r, rx, ry), dir);
}

SkPathFirstDirection SkPathPriv::ComputeFirstDirection(const SkPath& path) {
    auto convexity = path.getConvexityOrUnknown();
    if (SkPathConvexity_IsConvex(convexity)) {
        return SkPathConvexity_ToFirstDirection(convexity);
    }
    if (auto raw = SkPathPriv::Raw(path, SkResolveConvexity::kNo)) {
        return ComputeFirstDirection(*raw);
    }
    return SkPathFirstDirection::kUnknown;
}

SkPathData* SkPath::PeekErrorSingleton() {
    static SkPathData* gErrorSingleton = SkPathData::MakeNoCheck({}, {}, {}, {}, {}).release();
    return gErrorSingleton;
}

SkPath SkPath::MakeNullCheck(sk_sp<SkPathData> pdata, SkPathFillType ft, bool isVolatile) {
    if (!pdata) {
        pdata = sk_ref_sp(PeekErrorSingleton());
    }
    return SkPath(std::move(pdata), ft, isVolatile);
}

bool SkPathPriv::PerspectiveClip(const SkPath& path, const SkMatrix& matrix, SkPath* clippedPath) {
    if (!matrix.hasPerspective()) {
        return false;
    }
    auto raw = SkPathPriv::Raw(path, SkResolveConvexity::kNo);
    if (!raw) {
        *clippedPath = SkPath();
        return true;
    }
    auto* buf = skia_rs_path_perspective_clip(
            raw->fPoints.data(), raw->fPoints.size(),
            verbs_u8(raw->fVerbs), raw->fVerbs.size(),
            raw->fConics.data(), raw->fConics.size(),
            raw->fBounds,
            static_cast<uint8_t>(raw->fFillType),
            static_cast<uint8_t>(raw->fConvexity),
            raw->fSegmentMask,
            &matrix);
    if (!buf) {
        return false;
    }
    *clippedPath = path_from_rust_buf(buf);
    return true;
}

std::optional<SkPathRectInfo> SkPathPriv::IsSimpleRect(const SkPath& path, bool isSimpleFill) {
    auto pts = path.points();
    auto vbs = path.verbs();
    auto cns = path.conicWeights();
    auto res = skia_rs_path_is_simple_rect(
            pts.data(), pts.size(),
            verbs_u8(vbs), vbs.size(),
            cns.data(), cns.size(),
            path.getSegmentMasks(),
            path.isFinite(),
            isSimpleFill);
    if (!res.found) {
        return {};
    }
    return SkPathRectInfo{
        res.rect,
        static_cast<SkPathDirection>(res.direction),
        res.start_index,
    };
}

SkPathEdgeIter::SkPathEdgeIter(const SkPathRaw& raw) {
    fMoveToPtr = fPts = raw.fPoints.data();
    fVerbs = raw.fVerbs.data();
    fVerbsStop = fVerbs + raw.fVerbs.size();
    fConicWeights = raw.fConics.data();
    if (fConicWeights) {
        fConicWeights -= 1;
    }
    fNeedsCloseLine = false;
    fNextIsNewContour = false;
    SkDEBUGCODE(fIsConic = false;)
}

SkPathEdgeIter::SkPathEdgeIter(const SkPath& path)
    : SkPathEdgeIter(SkPathPriv::Raw(path, SkResolveConvexity::kNo).value_or(SkPathRaw::Empty())) {}

// ============================================================================
// SkPathBuilder.cpp
// ============================================================================

template <typename PtsArr, typename VbsArr, typename CnsArr>
static void apply_rust_buf_to_builder(SkPathBuilder& pb,
                                      SkiaRsPathBuf* buf,
                                      PtsArr& ptsArr,
                                      VbsArr& vbsArr,
                                      CnsArr& cnsArr,
                                      SkPathFillType& fillType,
                                      bool& isVolatile,
                                      SkPathConvexity& convexity,
                                      unsigned& segMask,
                                      int& lastMoveIdx,
                                      SkPathIsAType& isAType,
                                      SkPathIsAData& isAData) {
    (void)pb;
    if (!buf) {
        return;
    }
    size_t npts = 0, nvbs = 0, ncns = 0;
    const SkPoint* pts = skia_rs_path_buf_points(buf, &npts);
    const uint8_t* vbs = skia_rs_path_buf_verbs(buf, &nvbs);
    const float* cns = skia_rs_path_buf_conics(buf, &ncns);
    auto meta = skia_rs_path_buf_meta(buf);

    ptsArr.clear();
    if (npts > 0) {
        memcpy(ptsArr.push_back_n(npts), pts, npts * sizeof(SkPoint));
    }
    vbsArr.clear();
    if (nvbs > 0) {
        memcpy(vbsArr.push_back_n(nvbs), vbs, nvbs * sizeof(SkPathVerb));
    }
    cnsArr.clear();
    if (ncns > 0) {
        memcpy(cnsArr.push_back_n(ncns), cns, ncns * sizeof(float));
    }

    fillType = static_cast<SkPathFillType>(meta.fill_type);
    isVolatile = meta.is_volatile;
    convexity = static_cast<SkPathConvexity>(meta.convexity);
    segMask = meta.segment_mask;
    lastMoveIdx = meta.last_move_index;
    isAType = static_cast<SkPathIsAType>(meta.is_a_type);
    isAData.fDirection = static_cast<SkPathDirection>(meta.is_a_dir);
    isAData.fStartIndex = meta.is_a_start_index;

    skia_rs_path_buf_free(buf);
}

SkPathBuilder::SkPathBuilder() { this->reset(); }
SkPathBuilder::SkPathBuilder(const SkPathBuilder&) = default;
SkPathBuilder& SkPathBuilder::operator=(const SkPathBuilder&) = default;
SkPathBuilder::SkPathBuilder(SkPathBuilder&&) = default;
SkPathBuilder& SkPathBuilder::operator=(SkPathBuilder&&) = default;
SkPathBuilder::~SkPathBuilder() = default;

SkPathBuilder::SkPathBuilder(SkPathFillType ft) {
    this->reset();
    fFillType = ft;
}

SkPathBuilder::SkPathBuilder(const SkPath& src) { *this = src; }

SkPathBuilder& SkPathBuilder::operator=(const SkPath& src) {
    this->reset().setFillType(src.getFillType());
    this->setIsVolatile(src.isVolatile());
    if (src.isEmpty()) {
        return *this;
    }
    this->addRaw(src.fPathData->raw(src.getFillType(), SkResolveConvexity::kYes), Reserve::kExact);
    fLastMoveIndex = SkPathPriv::FindLastMoveToIndex(fVerbs, fPts.size());
    fType = src.fPathData->fType;
    fIsA = src.fPathData->fIsA;
    return *this;
}

SkPathBuilder& SkPathBuilder::reset() {
    fPts.clear();
    fVerbs.clear();
    fConicWeights.clear();
    fFillType = SkPathFillType::kDefault;
    fIsVolatile = false;
    fSegmentMask = 0;
    fLastMoveIndex = -1;
    fType = SkPathIsAType::kGeneral;
    fConvexity = SkPathConvexity::kUnknown;
    return *this;
}

bool SkPathBuilder::operator==(const SkPathBuilder& o) const {
    if (this == &o) {
        return true;
    }
    if (fSegmentMask != o.fSegmentMask || fFillType != o.fFillType) {
        return false;
    }
    return fVerbs == o.fVerbs && fPts == o.fPts && fConicWeights == o.fConicWeights;
}

void SkPathBuilder::incReserve(int extraPtCount, int extraVbCount, int extraCnCount) {
    fPts.reserve(Sk32_sat_add(fPts.size(), extraPtCount));
    fVerbs.reserve(Sk32_sat_add(fVerbs.size(), extraVbCount));
    fConicWeights.reserve(Sk32_sat_add(fConicWeights.size(), extraCnCount));
}

std::tuple<SkPoint*, SkScalar*> SkPathBuilder::growForVerbsInPath(const SkPath& path) {
    fSegmentMask |= path.getSegmentMasks();
    const SkSpan<const SkPathVerb> verbs = path.verbs();
    if (!verbs.empty()) {
        fType = SkPathIsAType::kGeneral;
        fVerbs.push_back_n(verbs.size(), verbs.data());
    }
    SkPoint* pts = nullptr;
    if (int numPts = path.countPoints()) {
        pts = fPts.push_back_n(numPts);
    }
    SkScalar* weights = nullptr;
    if (int numConics = path.conicWeights().size()) {
        weights = fConicWeights.push_back_n(numConics);
    }
    return {pts, weights};
}

SkPathBuilder& SkPathBuilder::moveTo(SkPoint pt) {
    if (!fVerbs.empty() && fVerbs.back() == SkPathVerb::kMove) {
        fPts.back() = pt;
    } else {
        fLastMoveIndex = SkToInt(fPts.size());
        fPts.push_back(pt);
        fVerbs.push_back(SkPathVerb::kMove);
        if (fType == SkPathIsAType::kOval || fType == SkPathIsAType::kRRect) {
            fType = SkPathIsAType::kGeneral;
        }
        fConvexity = SkPathConvexity::kUnknown;
    }
    return *this;
}

SkPathBuilder& SkPathBuilder::lineTo(SkPoint pt) {
    this->ensureMove();
    fPts.push_back(pt);
    fVerbs.push_back(SkPathVerb::kLine);
    fSegmentMask |= kLine_SkPathSegmentMask;
    return *this;
}

SkPathBuilder& SkPathBuilder::quadTo(SkPoint pt1, SkPoint pt2) {
    this->ensureMove();
    SkPoint* p = fPts.push_back_n(2);
    p[0] = pt1;
    p[1] = pt2;
    fVerbs.push_back(SkPathVerb::kQuad);
    fSegmentMask |= kQuad_SkPathSegmentMask;
    return *this;
}

SkPathBuilder& SkPathBuilder::conicTo(SkPoint pt1, SkPoint pt2, SkScalar w) {
    this->ensureMove();
    if (w <= 0) {
        return this->lineTo(pt2);
    }
    SkPoint* p = fPts.push_back_n(2);
    p[0] = pt1;
    p[1] = pt2;
    if (w == 1) {
        fVerbs.push_back(SkPathVerb::kQuad);
        fSegmentMask |= kQuad_SkPathSegmentMask;
    } else if (SkIsFinite(w)) {
        fVerbs.push_back(SkPathVerb::kConic);
        fConicWeights.push_back(w);
        fSegmentMask |= kConic_SkPathSegmentMask;
    } else {
        fVerbs.push_back(SkPathVerb::kLine);
        fVerbs.push_back(SkPathVerb::kLine);
        fSegmentMask |= kLine_SkPathSegmentMask;
    }
    return *this;
}

SkPathBuilder& SkPathBuilder::cubicTo(SkPoint pt1, SkPoint pt2, SkPoint pt3) {
    this->ensureMove();
    SkPoint* p = fPts.push_back_n(3);
    p[0] = pt1;
    p[1] = pt2;
    p[2] = pt3;
    fVerbs.push_back(SkPathVerb::kCubic);
    fSegmentMask |= kCubic_SkPathSegmentMask;
    return *this;
}

SkPathBuilder& SkPathBuilder::close() {
    if (!fVerbs.empty() && fVerbs.back() != SkPathVerb::kClose) {
        this->ensureMove();
        fVerbs.push_back(SkPathVerb::kClose);
    }
    return *this;
}

SkPathBuilder& SkPathBuilder::rMoveTo(SkVector pt) {
    SkPoint lastPt = {0, 0};
    if (!fPts.empty()) {
        lastPt = (fVerbs.back() == SkPathVerb::kClose) ? fPts[fLastMoveIndex] : fPts.back();
    }
    return this->moveTo(lastPt + pt);
}

SkPathBuilder& SkPathBuilder::rLineTo(SkVector p1) {
    this->ensureMove();
    return this->lineTo(fPts.back() + p1);
}

SkPathBuilder& SkPathBuilder::rQuadTo(SkVector p1, SkVector p2) {
    this->ensureMove();
    SkPoint base = fPts.back();
    return this->quadTo(base + p1, base + p2);
}

SkPathBuilder& SkPathBuilder::rConicTo(SkVector p1, SkVector p2, SkScalar w) {
    this->ensureMove();
    SkPoint base = fPts.back();
    return this->conicTo(base + p1, base + p2, w);
}

SkPathBuilder& SkPathBuilder::rCubicTo(SkVector p1, SkVector p2, SkVector p3) {
    this->ensureMove();
    SkPoint base = fPts.back();
    return this->cubicTo(base + p1, base + p2, base + p3);
}

SkPath SkPathBuilder::snapshot(const SkMatrix* mx) const {
    if (!mx) {
        mx = &SkMatrix::I();
    }
    sk_sp<SkPathData> pdata;
    if (auto raw = SkPathPriv::Raw(*this, SkResolveConvexity::kNo)) {
        pdata = SkPathData::MakeTransform(*raw, *mx);
    }
    if (pdata && fType != SkPathIsAType::kGeneral) {
        if (mx->rectStaysRect() && !pdata->bounds().isEmpty()) {
            auto [dir, start] = SkPathPriv::TransformDirAndStart(
                    *mx, fType == SkPathIsAType::kRRect, fIsA.fDirection, fIsA.fStartIndex);
            pdata->setupIsA(fType, dir, start);
        }
    }
    return SkPath::MakeNullCheck(std::move(pdata), fFillType, fIsVolatile);
}

SkPath SkPathBuilder::detach(const SkMatrix* mx) {
    auto path = this->snapshot(mx);
    this->reset();
    return path;
}

sk_sp<SkPathData> SkPathBuilder::snapshotData() const {
    if (fVerbs.size() <= 1) {
        return SkPathData::Empty();
    }
    switch (fType) {
        case SkPathIsAType::kGeneral:
            break;
        case SkPathIsAType::kOval:
            if (auto r = SkRect::Bounds(fPts)) {
                return SkPathData::Oval(*r, fIsA.fDirection, fIsA.fStartIndex);
            }
            return nullptr;
        case SkPathIsAType::kRRect:
            if (auto r = SkRect::Bounds(fPts)) {
                return SkPathData::RRect(SkPathPriv::DeduceRRectFromContour(*r, fPts, fVerbs),
                                         fIsA.fDirection, fIsA.fStartIndex);
            }
            return nullptr;
    }
    return SkPathData::Make(fPts, fVerbs, fConicWeights);
}

sk_sp<SkPathData> SkPathBuilder::detachData() {
    auto data = this->snapshotData();
    this->reset();
    return data;
}

SkPathBuilder& SkPathBuilder::arcTo(const SkRect& oval, SkScalar startAngle, SkScalar sweepAngle,
                                    bool forceMoveTo) {
    auto* buf = skia_rs_builder_arc_to_oval(
            fPts.data(), fPts.size(),
            verbs_u8(fVerbs), fVerbs.size(),
            fConicWeights.data(), fConicWeights.size(),
            static_cast<uint8_t>(fFillType), static_cast<uint8_t>(fConvexity),
            static_cast<uint8_t>(fSegmentMask), fLastMoveIndex,
            static_cast<uint8_t>(fType), static_cast<uint8_t>(fIsA.fDirection),
            fIsA.fStartIndex, fIsVolatile,
            oval, startAngle, sweepAngle, forceMoveTo);
    apply_rust_buf_to_builder(*this, buf, fPts, fVerbs, fConicWeights, fFillType, fIsVolatile,
                              fConvexity, fSegmentMask, fLastMoveIndex, fType, fIsA);
    return *this;
}

SkPathBuilder& SkPathBuilder::rArcTo(SkPoint r, SkScalar xAxisRotate, ArcSize largeArc,
                                     SkPathDirection sweep, SkVector dxdy) {
    const SkPoint currentPoint = this->getLastPt().value_or(SkPoint{0, 0});
    return this->arcTo(r, xAxisRotate, largeArc, sweep, currentPoint + dxdy);
}

SkPathBuilder& SkPathBuilder::addArc(const SkRect& oval, SkScalar startAngle, SkScalar sweepAngle) {
    auto* buf = skia_rs_builder_add_arc(
            fPts.data(), fPts.size(),
            verbs_u8(fVerbs), fVerbs.size(),
            fConicWeights.data(), fConicWeights.size(),
            static_cast<uint8_t>(fFillType), static_cast<uint8_t>(fConvexity),
            static_cast<uint8_t>(fSegmentMask), fLastMoveIndex,
            static_cast<uint8_t>(fType), static_cast<uint8_t>(fIsA.fDirection),
            fIsA.fStartIndex, fIsVolatile,
            oval, startAngle, sweepAngle);
    apply_rust_buf_to_builder(*this, buf, fPts, fVerbs, fConicWeights, fFillType, fIsVolatile,
                              fConvexity, fSegmentMask, fLastMoveIndex, fType, fIsA);
    return *this;
}

SkPathBuilder& SkPathBuilder::arcTo(SkPoint p1, SkPoint p2, SkScalar radius) {
    auto* buf = skia_rs_builder_arc_to_tangent(
            fPts.data(), fPts.size(),
            verbs_u8(fVerbs), fVerbs.size(),
            fConicWeights.data(), fConicWeights.size(),
            static_cast<uint8_t>(fFillType), static_cast<uint8_t>(fConvexity),
            static_cast<uint8_t>(fSegmentMask), fLastMoveIndex,
            static_cast<uint8_t>(fType), static_cast<uint8_t>(fIsA.fDirection),
            fIsA.fStartIndex, fIsVolatile,
            p1, p2, radius);
    apply_rust_buf_to_builder(*this, buf, fPts, fVerbs, fConicWeights, fFillType, fIsVolatile,
                              fConvexity, fSegmentMask, fLastMoveIndex, fType, fIsA);
    return *this;
}

SkPathBuilder& SkPathBuilder::arcTo(SkPoint rad, SkScalar angle, ArcSize arcLarge,
                                    SkPathDirection sweep, SkPoint endPt) {
    auto* buf = skia_rs_builder_arc_to_svg(
            fPts.data(), fPts.size(),
            verbs_u8(fVerbs), fVerbs.size(),
            fConicWeights.data(), fConicWeights.size(),
            static_cast<uint8_t>(fFillType), static_cast<uint8_t>(fConvexity),
            static_cast<uint8_t>(fSegmentMask), fLastMoveIndex,
            static_cast<uint8_t>(fType), static_cast<uint8_t>(fIsA.fDirection),
            fIsA.fStartIndex, fIsVolatile,
            rad, angle, static_cast<uint8_t>(arcLarge), static_cast<uint8_t>(sweep), endPt);
    apply_rust_buf_to_builder(*this, buf, fPts, fVerbs, fConicWeights, fFillType, fIsVolatile,
                              fConvexity, fSegmentMask, fLastMoveIndex, fType, fIsA);
    return *this;
}

SkPathIter SkPathBuilder::iter() const {
    return SkPathIter(fPts, fVerbs, fConicWeights);
}

SkPathBuilder& SkPathBuilder::addRaw(const SkPathRaw& raw, Reserve reserve) {
    if (reserve == Reserve::kGrow) {
        this->incReserve(raw.points().size(), raw.verbs().size(), raw.conics().size());
    } else {
        fPts.reserve_exact(Sk32_sat_add(fPts.size(), raw.points().size()));
        fVerbs.reserve_exact(Sk32_sat_add(fVerbs.size(), raw.verbs().size()));
        fConicWeights.reserve_exact(Sk32_sat_add(fConicWeights.size(), raw.conics().size()));
    }
    for (auto iter = raw.iter(); auto rec = iter.next();) {
        const auto pts = rec->fPoints;
        switch (rec->fVerb) {
            case SkPathVerb::kMove:  this->moveTo(pts[0]); break;
            case SkPathVerb::kLine:  this->lineTo(pts[1]); break;
            case SkPathVerb::kQuad:  this->quadTo(pts[1], pts[2]); break;
            case SkPathVerb::kConic: this->conicTo(pts[1], pts[2], rec->fConicWeight); break;
            case SkPathVerb::kCubic: this->cubicTo(pts[1], pts[2], pts[3]); break;
            case SkPathVerb::kClose: this->close(); break;
        }
    }
    auto has_trailing_move = [](SkSpan<const SkPathVerb> vbs) {
        return !vbs.empty() && vbs.back() == SkPathVerb::kMove;
    };
    if (has_trailing_move(raw.verbs()) && !has_trailing_move(this->verbs())) {
        this->moveTo(raw.points().back());
    }
    return *this;
}

static bool is_empty_or_moves(const SkSpan<const SkPathVerb>& verbs) {
    return verbs.empty() || (verbs.size() == 1 && verbs.back() == SkPathVerb::kMove);
}

SkPathBuilder& SkPathBuilder::addRect(const SkRect& rect, SkPathDirection dir, unsigned index) {
    const bool wasEmpty = is_empty_or_moves(fVerbs);
    this->addRaw(SkPathRawShapes::Rect(rect, dir, index), Reserve::kGrow);
    if (wasEmpty) {
        fConvexity = SkPathDirection_ToConvexity(dir);
    }
    return *this;
}

SkPathBuilder& SkPathBuilder::addOval(const SkRect& oval, SkPathDirection dir, unsigned index) {
    const bool wasEmpty = is_empty_or_moves(fVerbs);
    this->addRaw(SkPathRawShapes::Oval(oval, dir, index), Reserve::kGrow);
    if (wasEmpty) {
        fType = SkPathIsAType::kOval;
        fIsA.fDirection = dir;
        fIsA.fStartIndex = index % 4;
        fConvexity = SkPathDirection_ToConvexity(dir);
    }
    return *this;
}

SkPathBuilder& SkPathBuilder::addRRect(const SkRRect& rrect, SkPathDirection dir, unsigned index) {
    const SkRect& bounds = rrect.getBounds();
    auto [asType, newIndex] = SkPathPriv::SimplifyRRect(rrect, index);
    switch (asType) {
        case SkPathPriv::RRectAsEnum::kRect:
            return this->addRect(bounds, dir, newIndex);
        case SkPathPriv::RRectAsEnum::kOval:
            return this->addOval(bounds, dir, newIndex);
        case SkPathPriv::RRectAsEnum::kRRect:
            break;
    }
    const bool wasEmpty = is_empty_or_moves(fVerbs);
    this->addRaw(SkPathRawShapes::RRect(rrect, dir, index), Reserve::kGrow);
    if (wasEmpty) {
        fType = SkPathIsAType::kRRect;
        fIsA.fDirection = dir;
        fIsA.fStartIndex = index % 8;
        fConvexity = SkPathDirection_ToConvexity(dir);
    }
    return *this;
}

SkPathBuilder& SkPathBuilder::addCircle(SkPoint center, float r, SkPathDirection dir) {
    if (r >= 0) {
        this->addOval(SkRect::MakeLTRB(center.fX - r, center.fY - r, center.fX + r, center.fY + r),
                      dir);
    }
    return *this;
}

SkPathBuilder& SkPathBuilder::addPolygon(SkSpan<const SkPoint> pts, bool isClosed) {
    if (pts.empty()) {
        return *this;
    }
    this->moveTo(pts[0]);
    this->polylineTo(pts.last(pts.size() - 1));
    if (isClosed) {
        this->close();
    }
    return *this;
}

SkPathBuilder& SkPathBuilder::polylineTo(SkSpan<const SkPoint> pts) {
    if (!pts.empty()) {
        this->ensureMove();
        const auto count = pts.size();
        this->incReserve(count, count, 0);
        memcpy(fPts.push_back_n(count), pts.data(), count * sizeof(SkPoint));
        memset(fVerbs.push_back_n(count), (uint8_t)SkPathVerb::kLine, count);
        fSegmentMask |= kLine_SkPathSegmentMask;
    }
    return *this;
}

SkPathBuilder& SkPathBuilder::offset(SkScalar dx, SkScalar dy) {
    for (auto& p : fPts) {
        p += {dx, dy};
    }
    return *this;
}

SkPathBuilder& SkPathBuilder::addPath(const SkPath& path, SkScalar dx, SkScalar dy,
                                      SkPath::AddPathMode mode) {
    SkMatrix matrix = SkMatrix::Translate(dx, dy);
    return this->addPath(path, matrix, mode);
}

SkPathBuilder& SkPathBuilder::addPath(const SkPath& src, const SkMatrix& matrix,
                                      SkPath::AddPathMode mode) {
    if (src.isEmpty()) {
        return *this;
    }
    auto sp = src.points();
    auto sv = src.verbs();
    auto sc = src.conicWeights();
    auto* buf = skia_rs_builder_add_path(
            fPts.data(), fPts.size(),
            verbs_u8(fVerbs), fVerbs.size(),
            fConicWeights.data(), fConicWeights.size(),
            static_cast<uint8_t>(fFillType), static_cast<uint8_t>(fConvexity),
            static_cast<uint8_t>(fSegmentMask), fLastMoveIndex,
            static_cast<uint8_t>(fType), static_cast<uint8_t>(fIsA.fDirection),
            fIsA.fStartIndex, fIsVolatile,
            sp.data(), sp.size(),
            verbs_u8(sv), sv.size(),
            sc.data(), sc.size(),
            static_cast<uint8_t>(src.getSegmentMasks()),
            src.isFinite(),
            static_cast<uint8_t>(src.fPathData->fType),
            static_cast<uint8_t>(src.fPathData->fIsA.fDirection),
            src.fPathData->fIsA.fStartIndex,
            static_cast<uint8_t>(src.getConvexityOrUnknown()),
            src.isVolatile(),
            &matrix,
            static_cast<uint8_t>(mode));
    apply_rust_buf_to_builder(*this, buf, fPts, fVerbs, fConicWeights, fFillType, fIsVolatile,
                              fConvexity, fSegmentMask, fLastMoveIndex, fType, fIsA);
    return *this;
}

SkPathBuilder& SkPathBuilder::privateReversePathTo(const SkPath& path) {
    if (path.verbs().empty()) {
        return *this;
    }
    auto sp = path.points();
    auto sv = path.verbs();
    auto sc = path.conicWeights();
    auto* buf = skia_rs_builder_reverse_path_to(
            fPts.data(), fPts.size(),
            verbs_u8(fVerbs), fVerbs.size(),
            fConicWeights.data(), fConicWeights.size(),
            static_cast<uint8_t>(fFillType), static_cast<uint8_t>(fConvexity),
            static_cast<uint8_t>(fSegmentMask), fLastMoveIndex,
            static_cast<uint8_t>(fType), static_cast<uint8_t>(fIsA.fDirection),
            fIsA.fStartIndex, fIsVolatile,
            sp.data(), sp.size(),
            verbs_u8(sv), sv.size(),
            sc.data(), sc.size());
    apply_rust_buf_to_builder(*this, buf, fPts, fVerbs, fConicWeights, fFillType, fIsVolatile,
                              fConvexity, fSegmentMask, fLastMoveIndex, fType, fIsA);
    return *this;
}

SkPathBuilder& SkPathBuilder::privateReverseAddPath(const SkPath& src) {
    if (src.verbs().empty()) {
        return *this;
    }
    auto sp = src.points();
    auto sv = src.verbs();
    auto sc = src.conicWeights();
    auto* buf = skia_rs_builder_reverse_add_path(
            fPts.data(), fPts.size(),
            verbs_u8(fVerbs), fVerbs.size(),
            fConicWeights.data(), fConicWeights.size(),
            static_cast<uint8_t>(fFillType), static_cast<uint8_t>(fConvexity),
            static_cast<uint8_t>(fSegmentMask), fLastMoveIndex,
            static_cast<uint8_t>(fType), static_cast<uint8_t>(fIsA.fDirection),
            fIsA.fStartIndex, fIsVolatile,
            sp.data(), sp.size(),
            verbs_u8(sv), sv.size(),
            sc.data(), sc.size());
    apply_rust_buf_to_builder(*this, buf, fPts, fVerbs, fConicWeights, fFillType, fIsVolatile,
                              fConvexity, fSegmentMask, fLastMoveIndex, fType, fIsA);
    return *this;
}

std::optional<SkPoint> SkPathBuilder::getLastPt() const {
    if (!fPts.empty()) {
        return fPts.back();
    }
    return std::nullopt;
}

#ifdef SK_SUPPORT_LEGACY_PATHBUILDER_SETLASTPT
void SkPathBuilder::setLastPt(SkPoint pt) {
    size_t count = fPts.size();
    if (count == 0) {
        this->moveTo(pt);
    } else {
        this->setPoint(count - 1, pt);
    }
}
#endif

void SkPathBuilder::setPoint(size_t index, SkPoint p) {
    if (index < static_cast<size_t>(fPts.size())) {
        fPts[index] = p;
        fType = SkPathIsAType::kGeneral;
    }
}

SkPathBuilder& SkPathBuilder::transform(const SkMatrix& matrix) {
    if (matrix.isIdentity() || this->isEmpty()) {
        return *this;
    }
    auto* buf = skia_rs_builder_transform(
            fPts.data(), fPts.size(),
            verbs_u8(fVerbs), fVerbs.size(),
            fConicWeights.data(), fConicWeights.size(),
            static_cast<uint8_t>(fFillType), static_cast<uint8_t>(fConvexity),
            static_cast<uint8_t>(fSegmentMask), fLastMoveIndex,
            static_cast<uint8_t>(fType), static_cast<uint8_t>(fIsA.fDirection),
            fIsA.fStartIndex, fIsVolatile,
            &matrix);
    apply_rust_buf_to_builder(*this, buf, fPts, fVerbs, fConicWeights, fFillType, fIsVolatile,
                              fConvexity, fSegmentMask, fLastMoveIndex, fType, fIsA);
    return *this;
}

std::optional<SkRect> SkPathBuilder::computeFiniteBounds() const {
    SkRect r;
    if (skia_rs_path_trimmed_bounds(
                fPts.data(), fPts.size(), verbs_u8(fVerbs), fVerbs.size(), &r)) {
        return r;
    }
    return {};
}

std::optional<SkRect> SkPathBuilder::computeTightBounds() const {
    if (!this->isFinite()) {
        return {};
    }
    return SkPathPriv::ComputeTightBounds(this->points(), this->verbs(), this->conicWeights());
}

bool SkPathBuilder::isFinite() const {
    for (auto p : fPts) {
        if (!p.isFinite()) {
            return false;
        }
    }
    return true;
}

bool SkPathBuilder::isZeroLengthSincePoint(int startPtIndex) const {
    return skia_rs_path_is_zero_length_since_point(
            fPts.data(), fPts.size(), startPtIndex < 0 ? 0 : static_cast<size_t>(startPtIndex));
}

bool SkPathBuilder::contains(SkPoint p) const {
    const auto raw = SkPathPriv::Raw(*this, SkResolveConvexity::kNo);
    return raw.has_value() && SkPathPriv::Contains(*raw, p);
}

// ============================================================================
// SkPathDump.cpp
// ============================================================================

void SkPath::dump(SkWStream* wStream, bool dumpAsHex) const {
    auto pts = this->points();
    auto vbs = this->verbs();
    auto cns = this->conicWeights();
    SkString str = take_rust_string(skia_rs_path_dump_to_string(
            static_cast<uint8_t>(this->getFillType()),
            pts.data(), pts.size(),
            verbs_u8(vbs), vbs.size(),
            cns.data(), cns.size(),
            dumpAsHex, false));
    if (wStream) {
        wStream->writeText(str.c_str());
    } else if (!str.isEmpty()) {
        SkDebugf("%s", str.c_str());
    }
}

SkString SkPathBuilder::dumpToString(DumpFormat format) const {
    auto pts = this->points();
    auto vbs = this->verbs();
    auto cns = this->conicWeights();
    return take_rust_string(skia_rs_path_dump_to_string(
            static_cast<uint8_t>(this->fillType()),
            pts.data(), pts.size(),
            verbs_u8(vbs), vbs.size(),
            cns.data(), cns.size(),
            format == DumpFormat::kHex, true));
}

void SkPathBuilder::dump(DumpFormat format) const {
    SkDebugf("%s", dumpToString(format).c_str());
}

// ============================================================================
// SkPath_interpolate.cpp
// ============================================================================

bool SkPath::isInterpolatable(const SkPath& compare) const {
    auto fv = this->verbs();
    auto fc = this->conicWeights();
    auto tv = compare.verbs();
    auto tc = compare.conicWeights();
    return skia_rs_path_can_interpolate(
            this->points().size(), verbs_u8(fv), fv.size(), fc.data(), fc.size(),
            compare.points().size(), verbs_u8(tv), tv.size(), tc.data(), tc.size());
}

SkPath SkPath::makeInterpolate(const SkPath& ending, SkScalar weight) const {
    SkPath out;
    if (this->interpolate(ending, weight, &out)) {
        return out;
    }
    return SkPath();
}

bool SkPath::interpolate(const SkPath& ending, SkScalar weight, SkPath* out) const {
    if (!this->isInterpolatable(ending)) {
        return false;
    }
    auto fromPts = this->points();
    auto toPts = ending.points();
    std::vector<SkPoint> dst(fromPts.size());
    skia_rs_path_interpolate_points(
            fromPts.data(), toPts.data(), fromPts.size(), 1.0f - weight, dst.data());
    *out = SkPath::Raw({dst.data(), dst.size()}, this->verbs(), this->conicWeights(),
                       this->getFillType());
    return true;
}

// ============================================================================
// SkPath_serial.cpp
// ============================================================================

static bool extract_rrect_serial_info(const SkPath& path,
                                      SkRRect* outRR,
                                      uint8_t* outDir,
                                      uint32_t* outStart) {
    if (auto oinfo = SkPathPriv::IsOval(path)) {
        outRR->setOval(oinfo->fBounds);
        *outDir = static_cast<uint8_t>(oinfo->fDirection);
        *outStart = oinfo->fStartIndex * 2;
        return true;
    }
    if (auto rinfo = SkPathPriv::IsRRect(path)) {
        *outRR = rinfo->fRRect;
        *outDir = static_cast<uint8_t>(rinfo->fDirection);
        *outStart = rinfo->fStartIndex;
        return true;
    }
    return false;
}

size_t SkPath::writeToMemoryAsRRect(void* storage) const {
    SkRRect rr;
    uint8_t dir = 0;
    uint32_t start = 0;
    if (!extract_rrect_serial_info(*this, &rr, &dir, &start)) {
        return 0;
    }
    size_t needed = skia_rs_path_write_to_memory_as_rrect(
            static_cast<uint8_t>(fFillType), true, &rr, dir, start, nullptr, 0);
    if (!storage) {
        return needed;
    }
    return skia_rs_path_write_to_memory_as_rrect(
            static_cast<uint8_t>(fFillType), true, &rr, dir, start,
            static_cast<uint8_t*>(storage), needed);
}

size_t SkPath::writeToMemory(void* storage) const {
    SkDEBUGCODE(this->validate();)
    SkRRect rr;
    uint8_t dir = 0;
    uint32_t start = 0;
    bool hasRR = extract_rrect_serial_info(*this, &rr, &dir, &start);
    auto pts = this->points();
    auto vbs = this->verbs();
    auto cns = this->conicWeights();
    size_t needed = skia_rs_path_write_to_memory(
            static_cast<uint8_t>(fFillType),
            pts.data(), pts.size(),
            verbs_u8(vbs), vbs.size(),
            cns.data(), cns.size(),
            hasRR, hasRR ? &rr : nullptr, dir, start,
            nullptr, 0);
    if (!storage || needed == 0) {
        return needed;
    }
    return skia_rs_path_write_to_memory(
            static_cast<uint8_t>(fFillType),
            pts.data(), pts.size(),
            verbs_u8(vbs), vbs.size(),
            cns.data(), cns.size(),
            hasRR, hasRR ? &rr : nullptr, dir, start,
            static_cast<uint8_t*>(storage), needed);
}

sk_sp<SkData> SkPath::serialize() const {
    size_t size = this->writeToMemory(nullptr);
    sk_sp<SkData> data = SkData::MakeUninitialized(size);
    this->writeToMemory(data->writable_data());
    return data;
}

std::optional<SkPath> SkPath::ReadFromMemory(const void* storage, size_t length,
                                             size_t* bytesRead) {
    size_t localBytes = 0;
    if (!bytesRead) {
        bytesRead = &localBytes;
    }
    auto res = skia_rs_path_read_from_memory(static_cast<const uint8_t*>(storage), length);
    if (res.kind == 0) {
        *bytesRead = 0;
        return {};
    }
    *bytesRead = res.bytes_read;
    if (res.kind == 2) {
        SkPath path = SkPath::RRect(res.rrect, static_cast<SkPathDirection>(res.rrect_dir),
                                    res.rrect_start);
        path.setFillType(static_cast<SkPathFillType>(res.fill_type));
        return path;
    }
    return path_from_rust_buf(res.general_buf);
}
