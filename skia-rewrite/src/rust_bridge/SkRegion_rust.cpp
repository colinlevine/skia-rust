/*
 * Safe Rust bridge for src/core/SkRegion.cpp (Layer 2.3 skia-rs-region).
 */

#include "include/core/SkRegion.h"
#include "src/core/SkRegionPriv.h"

#include "include/private/SkMalloc.h"
#include "include/private/SkTemplates.h"
#include "include/private/SkTo.h"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <functional>
#include <utility>

static_assert(sizeof(SkIRect) == 16);

extern "C" {
struct SkiaRsRegionRunsBuf;

struct SkiaRsRunBoundsInfo {
    SkIRect bounds;
    int32_t y_span_count;
    int32_t interval_count;
};
static_assert(sizeof(SkiaRsRunBoundsInfo) == 24);

const int32_t* skia_rs_region_runs_buf_data(const SkiaRsRegionRunsBuf* buf, size_t* out_len);
void skia_rs_region_runs_buf_free(SkiaRsRegionRunsBuf* buf);

void skia_rs_region_build_rect_runs(const SkIRect* bounds, int32_t* out_runs);
bool skia_rs_region_runs_are_a_rect(const int32_t* runs, int32_t count, SkIRect* out_bounds);
bool skia_rs_region_complex_contains_xy(const int32_t* runs, size_t run_count, int32_t x, int32_t y);
bool skia_rs_region_complex_contains_rect(const int32_t* runs, size_t run_count, const SkIRect* r);
bool skia_rs_region_complex_intersects_rect(const int32_t* runs, size_t run_count, const SkIRect* sect);
int32_t skia_rs_region_pin_offset_s32(int32_t min, int32_t max, int32_t offset);
void skia_rs_region_translate_runs(
        const int32_t* src, int32_t* dst, size_t run_count, int32_t dx, int32_t dy);
void skia_rs_region_compute_run_bounds(
        const int32_t* runs, size_t run_count, SkiaRsRunBoundsInfo* out_info);
bool skia_rs_region_validate_runs(
        const int32_t* runs, size_t run_count, const SkIRect* given_bounds,
        int32_t y_span_count, int32_t interval_count);
int32_t skia_rs_region_normalize_runs(
        int32_t* runs, size_t count, SkIRect* out_bounds, int32_t* out_y_spans,
        int32_t* out_intervals, SkiaRsRegionRunsBuf** out_buf);
int32_t skia_rs_region_operate_runs(
        const int32_t* a_runs, size_t a_len, const int32_t* b_runs, size_t b_len,
        int32_t op, bool want_result, SkIRect* out_bounds, int32_t* out_y_spans,
        int32_t* out_intervals, SkiaRsRegionRunsBuf** out_buf);
int32_t skia_rs_region_set_rects(
        const SkIRect* rects, size_t count, SkIRect* out_bounds, int32_t* out_y_spans,
        int32_t* out_intervals, SkiaRsRegionRunsBuf** out_buf);
bool skia_rs_region_iter_next(
        const int32_t* runs, size_t run_count, size_t offset, SkIRect* rect,
        size_t* out_next_offset);
bool skia_rs_region_spanerator_init(
        const int32_t* runs, size_t run_count, int32_t y, int32_t left, int32_t right,
        size_t* out_offset);
void skia_rs_region_visit_spans(
        const int32_t* runs, size_t run_count,
        void (*cb)(const SkIRect*, void*), void* ctx);
size_t skia_rs_region_serialized_size(bool is_empty, bool is_rect, size_t run_count);
size_t skia_rs_region_write_to_memory(
        bool is_empty, bool is_rect, const SkIRect* bounds, int32_t y_span_count,
        int32_t interval_count, const int32_t* runs, size_t run_count,
        uint8_t* dst, size_t dst_len);
size_t skia_rs_region_read_from_memory(
        const uint8_t* src, size_t src_len, int32_t* out_kind, SkIRect* out_bounds,
        int32_t* out_y_spans, int32_t* out_intervals, SkiaRsRegionRunsBuf** out_buf);
}  // extern "C"

namespace {

struct RunsBufGuard {
    SkiaRsRegionRunsBuf* ptr = nullptr;
    ~RunsBufGuard() {
        if (ptr) {
            skia_rs_region_runs_buf_free(ptr);
        }
    }
};

static const SkRegionPriv::RunType* get_runs_with_len(
        const SkRegion& rgn, SkRegionPriv::RunType tmp[7], size_t* out_len) {
    if (rgn.isEmpty()) {
        tmp[0] = SkRegion_kRunTypeSentinel;
        *out_len = 1;
        return tmp;
    }
    if (rgn.isRect()) {
        skia_rs_region_build_rect_runs(&rgn.getBounds(), tmp);
        *out_len = 7;
        return tmp;
    }
    // Complex case handled by caller with direct RunHead access
    *out_len = 0;
    return nullptr;
}

}  // namespace

#define SkRegion_gRectRunHeadPtr    nullptr

bool SkRegion::RunsAreARect(const SkRegion::RunType runs[], int count, SkIRect* bounds) {
    return skia_rs_region_runs_are_a_rect(runs, count, bounds);
}

SkRegion::SkRegion() {
    fBounds.setEmpty();
    fRunHead = emptyRunHeadPtr();
}

SkRegion::SkRegion(const SkRegion& src) {
    fRunHead = emptyRunHeadPtr();
    fBounds.setEmpty();
    this->setRegion(src);
}

SkRegion::SkRegion(const SkIRect& rect) {
    fRunHead = emptyRunHeadPtr();
    this->setRect(rect);
}

SkRegion::~SkRegion() {
    this->freeRuns();
}

void SkRegion::freeRuns() {
    if (this->isComplex()) {
        SkASSERT(fRunHead->fRefCnt >= 1);
        if (--fRunHead->fRefCnt == 0) {
            sk_free(fRunHead);
        }
    }
}

void SkRegion::allocateRuns(int count, int ySpanCount, int intervalCount) {
    fRunHead = RunHead::Alloc(count, ySpanCount, intervalCount);
}

void SkRegion::allocateRuns(int count) {
    fRunHead = RunHead::Alloc(count);
}

void SkRegion::allocateRuns(const RunHead& head) {
    fRunHead = RunHead::Alloc(head.fRunCount,
                              head.getYSpanCount(),
                              head.getIntervalCount());
}

SkRegion& SkRegion::operator=(const SkRegion& src) {
    (void)this->setRegion(src);
    return *this;
}

void SkRegion::swap(SkRegion& other) {
    using std::swap;
    swap(fBounds, other.fBounds);
    swap(fRunHead, other.fRunHead);
}

int SkRegion::computeRegionComplexity() const {
    if (this->isEmpty()) {
        return 0;
    } else if (this->isRect()) {
        return 1;
    }
    return fRunHead->getIntervalCount();
}

bool SkRegion::setEmpty() {
    this->freeRuns();
    fBounds.setEmpty();
    fRunHead = emptyRunHeadPtr();
    return false;
}

bool SkRegion::setRect(const SkIRect& r) {
    if (r.isEmpty() ||
        SkRegion_kRunTypeSentinel == r.fRight ||
        SkRegion_kRunTypeSentinel == r.fBottom) {
        return this->setEmpty();
    }
    this->freeRuns();
    fBounds = r;
    fRunHead = SkRegion_gRectRunHeadPtr;
    return true;
}

bool SkRegion::setRegion(const SkRegion& src) {
    if (this != &src) {
        this->freeRuns();
        fBounds = src.fBounds;
        fRunHead = src.fRunHead;
        if (this->isComplex()) {
            fRunHead->fRefCnt++;
        }
    }
    return fRunHead != emptyRunHeadPtr();
}

bool SkRegion::op(const SkIRect& rect, const SkRegion& rgn, Op op) {
    SkRegion tmp(rect);
    return this->op(tmp, rgn, op);
}

bool SkRegion::op(const SkRegion& rgn, const SkIRect& rect, Op op) {
    SkRegion tmp(rect);
    return this->op(rgn, tmp, op);
}

#ifdef SK_BUILD_FOR_ANDROID_FRAMEWORK
char* SkRegion::toString() {
    Iterator iter(*this);
    int count = 0;
    while (!iter.done()) {
        count++;
        iter.next();
    }
    const int max = (count * ((11 * 4) + 5)) + 11 + 1;
    char* result = (char*)sk_malloc_throw(max);
    count = snprintf(result, max, "SkRegion(");
    iter.reset(*this);
    while (!iter.done()) {
        const SkIRect& r = iter.rect();
        count += snprintf(result + count, max - count, "(%d,%d,%d,%d)",
                          r.fLeft, r.fTop, r.fRight, r.fBottom);
        iter.next();
    }
    count += snprintf(result + count, max - count, ")");
    return result;
}
#endif

int SkRegion::count_runtype_values(int* itop, int* ibot) const {
    int maxT;
    if (this->isRect()) {
        maxT = 2;
    } else {
        SkASSERT(this->isComplex());
        maxT = fRunHead->getIntervalCount() * 2;
    }
    *itop = fBounds.fTop;
    *ibot = fBounds.fBottom;
    return maxT;
}

bool SkRegion::setRuns(RunType runs[], int count) {
    SkDEBUGCODE(SkRegionPriv::Validate(*this));
    SkASSERT(count > 0);
    if (count <= 2) {
        return this->setEmpty();
    }

    SkIRect bounds;
    int32_t ySpans = 0, intervals = 0;
    RunsBufGuard guard;
    int32_t kind = skia_rs_region_normalize_runs(
            runs, static_cast<size_t>(count), &bounds, &ySpans, &intervals, &guard.ptr);
    if (kind == 0) {
        return this->setEmpty();
    }
    if (kind == 1) {
        return this->setRect(bounds);
    }

    size_t len = 0;
    const int32_t* data = skia_rs_region_runs_buf_data(guard.ptr, &len);
    SkRegion tmp;
    tmp.allocateRuns(SkToInt(len), ySpans, intervals);
    memcpy(tmp.fRunHead->writable_runs(), data, len * sizeof(RunType));
    tmp.fBounds = bounds;
    this->swap(tmp);
    SkDEBUGCODE(SkRegionPriv::Validate(*this));
    return true;
}

void SkRegion::BuildRectRuns(const SkIRect& bounds, RunType runs[kRectRegionRuns]) {
    skia_rs_region_build_rect_runs(&bounds, runs);
}

bool SkRegion::contains(int32_t x, int32_t y) const {
    SkDEBUGCODE(SkRegionPriv::Validate(*this));
    if (!fBounds.contains(x, y)) {
        return false;
    }
    if (this->isRect()) {
        return true;
    }
    SkASSERT(this->isComplex());
    return skia_rs_region_complex_contains_xy(
            fRunHead->readonly_runs(), static_cast<size_t>(fRunHead->fRunCount), x, y);
}

bool SkRegion::contains(const SkIRect& r) const {
    SkDEBUGCODE(SkRegionPriv::Validate(*this));
    if (this->isEmpty() || r.isEmpty()) {
        return false;
    }
    if (!fBounds.containsNoEmptyCheck(r)) {
        return false;
    }
    if (this->isRect()) {
        return true;
    }
    SkASSERT(this->isComplex());
    return skia_rs_region_complex_contains_rect(
            fRunHead->readonly_runs(), static_cast<size_t>(fRunHead->fRunCount), &r);
}

bool SkRegion::contains(const SkRegion& rgn) const {
    SkDEBUGCODE(SkRegionPriv::Validate(*this));
    SkDEBUGCODE(SkRegionPriv::Validate(rgn));
    if (this->isEmpty() || rgn.isEmpty()) {
        return false;
    }
    if (this->isRect()) {
        return fBounds.containsNoEmptyCheck(rgn.fBounds);
    }
    if (this->fRunHead == rgn.fRunHead) {
        return true;
    }
    if (!fBounds.containsNoEmptyCheck(rgn.fBounds)) {
        return false;
    }
    if (rgn.isRect()) {
        return this->contains(rgn.getBounds());
    }
    return !Oper(rgn, *this, kDifference_Op, nullptr);
}

const SkRegion::RunType* SkRegion::getRuns(RunType tmpStorage[], int* intervals) const {
    SkASSERT(tmpStorage && intervals);
    const RunType* runs = tmpStorage;
    if (this->isEmpty()) {
        tmpStorage[0] = SkRegion_kRunTypeSentinel;
        *intervals = 0;
    } else if (this->isRect()) {
        BuildRectRuns(fBounds, tmpStorage);
        *intervals = 1;
    } else {
        runs = fRunHead->readonly_runs();
        *intervals = fRunHead->getIntervalCount();
    }
    return runs;
}

bool SkRegion::intersects(const SkIRect& r) const {
    SkDEBUGCODE(SkRegionPriv::Validate(*this));
    if (this->isEmpty() || r.isEmpty()) {
        return false;
    }
    SkIRect sect;
    if (!sect.intersect(fBounds, r)) {
        return false;
    }
    if (this->isRect()) {
        return true;
    }
    SkASSERT(this->isComplex());
    return skia_rs_region_complex_intersects_rect(
            fRunHead->readonly_runs(), static_cast<size_t>(fRunHead->fRunCount), &sect);
}

bool SkRegion::intersects(const SkRegion& rgn) const {
    if (this->isEmpty() || rgn.isEmpty()) {
        return false;
    }
    if (!SkIRect::Intersects(fBounds, rgn.fBounds)) {
        return false;
    }
    bool weAreARect = this->isRect();
    bool theyAreARect = rgn.isRect();
    if (weAreARect && theyAreARect) {
        return true;
    }
    if (weAreARect) {
        return rgn.intersects(this->getBounds());
    }
    if (theyAreARect) {
        return this->intersects(rgn.getBounds());
    }
    return Oper(*this, rgn, kIntersect_Op, nullptr);
}

bool SkRegion::operator==(const SkRegion& b) const {
    SkDEBUGCODE(SkRegionPriv::Validate(*this));
    SkDEBUGCODE(SkRegionPriv::Validate(b));
    if (this == &b) {
        return true;
    }
    if (fBounds != b.fBounds) {
        return false;
    }
    const RunHead* ah = fRunHead;
    const RunHead* bh = b.fRunHead;
    if (ah == bh) {
        return true;
    }
    if (!this->isComplex() || !b.isComplex()) {
        return false;
    }
    return ah->fRunCount == bh->fRunCount &&
           !memcmp(ah->readonly_runs(), bh->readonly_runs(),
                   ah->fRunCount * sizeof(RunType));
}

void SkRegion::translate(int dx, int dy, SkRegion* dst) const {
    SkDEBUGCODE(SkRegionPriv::Validate(*this));
    if (nullptr == dst) {
        return;
    }
    if (this->isEmpty()) {
        dst->setEmpty();
        return;
    }
    dx = skia_rs_region_pin_offset_s32(fBounds.fLeft, fBounds.fRight, dx);
    dy = skia_rs_region_pin_offset_s32(fBounds.fTop, fBounds.fBottom, dy);

    if (this->isRect()) {
        dst->setRect(fBounds.makeOffset(dx, dy));
    } else {
        if (this == dst) {
            dst->fRunHead = dst->fRunHead->ensureWritable();
        } else {
            SkRegion tmp;
            tmp.allocateRuns(*fRunHead);
            SkASSERT(tmp.isComplex());
            tmp.fBounds = fBounds;
            dst->swap(tmp);
        }
        dst->fBounds.offset(dx, dy);
        skia_rs_region_translate_runs(
                fRunHead->readonly_runs(),
                dst->fRunHead->writable_runs(),
                static_cast<size_t>(fRunHead->fRunCount),
                dx, dy);
    }
    SkDEBUGCODE(SkRegionPriv::Validate(*this));
}

bool SkRegion::setRects(SkSpan<const SkIRect> rects) {
    if (rects.empty()) {
        return this->setEmpty();
    }
    SkIRect bounds;
    int32_t ySpans = 0, intervals = 0;
    RunsBufGuard guard;
    int32_t kind = skia_rs_region_set_rects(
            rects.data(), rects.size(), &bounds, &ySpans, &intervals, &guard.ptr);
    if (kind == 0) {
        return this->setEmpty();
    }
    if (kind == 1) {
        return this->setRect(bounds);
    }
    size_t len = 0;
    const int32_t* data = skia_rs_region_runs_buf_data(guard.ptr, &len);
    SkRegion tmp;
    tmp.allocateRuns(SkToInt(len), ySpans, intervals);
    memcpy(tmp.fRunHead->writable_runs(), data, len * sizeof(RunType));
    tmp.fBounds = bounds;
    this->swap(tmp);
    SkDEBUGCODE(SkRegionPriv::Validate(*this));
    return true;
}

bool SkRegion::Oper(const SkRegion& rgnaOrig, const SkRegion& rgnbOrig, Op op, SkRegion* result) {
    SkASSERT((unsigned)op < kOpCount);

    if (kReplace_Op == op) {
        if (result) {
            *result = rgnbOrig;
        }
        return !rgnbOrig.isEmpty();
    }

    const SkRegion* rgna = &rgnaOrig;
    const SkRegion* rgnb = &rgnbOrig;
    if (kReverseDifference_Op == op) {
        std::swap(rgna, rgnb);
        op = kDifference_Op;
    }

    // Fast-path empty & disjoint checks matching Skia's gOpPairs table
    if (rgna->isEmpty()) {
        if (op == kUnion_Op || op == kXOR_Op) {
            if (result) {
                *result = *rgnb;
            }
            return !rgnb->isEmpty();
        }
        if (result) {
            result->setEmpty();
        }
        return false;
    }
    if (rgnb->isEmpty()) {
        if (op == kDifference_Op || op == kUnion_Op || op == kXOR_Op) {
            if (result) {
                *result = *rgna;
            }
            return true;
        }
        if (result) {
            result->setEmpty();
        }
        return false;
    }

    if (op == kIntersect_Op) {
        SkIRect sect;
        if (!sect.intersect(rgna->fBounds, rgnb->fBounds)) {
            if (result) {
                result->setEmpty();
            }
            return false;
        }
        if (rgna->isRect() && rgnb->isRect()) {
            if (result) {
                result->setRect(sect);
            }
            return true;
        }
    } else if (op == kDifference_Op) {
        if (!SkIRect::Intersects(rgna->fBounds, rgnb->fBounds)) {
            if (result) {
                *result = *rgna;
            }
            return true;
        }
        if (rgnb->isRect() && rgnb->fBounds.containsNoEmptyCheck(rgna->fBounds)) {
            if (result) {
                result->setEmpty();
            }
            return false;
        }
    } else if (op == kUnion_Op) {
        if (rgna->isRect() && rgna->fBounds.containsNoEmptyCheck(rgnb->fBounds)) {
            if (result) {
                *result = *rgna;
            }
            return true;
        }
        if (rgnb->isRect() && rgnb->fBounds.containsNoEmptyCheck(rgna->fBounds)) {
            if (result) {
                *result = *rgnb;
            }
            return true;
        }
    }

    RunType aStorage[kRectRegionRuns];
    RunType bStorage[kRectRegionRuns];
    size_t aLen = 0, bLen = 0;
    const RunType* aRuns = rgna->isComplex()
            ? (aLen = static_cast<size_t>(rgna->fRunHead->fRunCount),
               rgna->fRunHead->readonly_runs())
            : get_runs_with_len(*rgna, aStorage, &aLen);
    const RunType* bRuns = rgnb->isComplex()
            ? (bLen = static_cast<size_t>(rgnb->fRunHead->fRunCount),
               rgnb->fRunHead->readonly_runs())
            : get_runs_with_len(*rgnb, bStorage, &bLen);

    SkIRect bounds;
    int32_t ySpans = 0, intervals = 0;
    RunsBufGuard guard;
    int32_t kind = skia_rs_region_operate_runs(
            aRuns, aLen, bRuns, bLen, static_cast<int32_t>(op), result != nullptr,
            &bounds, &ySpans, &intervals, &guard.ptr);

    if (kind == 0) {
        if (result) {
            result->setEmpty();
        }
        return false;
    }
    if (!result) {
        return true;
    }
    if (kind == 1) {
        return result->setRect(bounds);
    }
    size_t len = 0;
    const int32_t* data = skia_rs_region_runs_buf_data(guard.ptr, &len);
    SkRegion tmp;
    tmp.allocateRuns(SkToInt(len), ySpans, intervals);
    memcpy(tmp.fRunHead->writable_runs(), data, len * sizeof(RunType));
    tmp.fBounds = bounds;
    result->swap(tmp);
    SkDEBUGCODE(SkRegionPriv::Validate(*result));
    return true;
}

bool SkRegion::op(const SkRegion& rgna, const SkRegion& rgnb, Op op) {
    SkDEBUGCODE(SkRegionPriv::Validate(*this));
    return SkRegion::Oper(rgna, rgnb, op, this);
}

size_t SkRegion::writeToMemory(void* storage) const {
    const bool isEmpty = this->isEmpty();
    const bool isRect = this->isRect();
    const size_t runCount = this->isComplex() ? static_cast<size_t>(fRunHead->fRunCount) : 0;
    const size_t needed = skia_rs_region_serialized_size(isEmpty, isRect, runCount);
    if (nullptr == storage) {
        return needed;
    }
    return skia_rs_region_write_to_memory(
            isEmpty, isRect, &fBounds,
            this->isComplex() ? fRunHead->getYSpanCount() : 0,
            this->isComplex() ? fRunHead->getIntervalCount() : 0,
            this->isComplex() ? fRunHead->readonly_runs() : nullptr,
            runCount,
            static_cast<uint8_t*>(storage),
            needed);
}

size_t SkRegion::readFromMemory(const void* storage, size_t length) {
    int32_t kind = 0;
    SkIRect bounds;
    int32_t ySpans = 0, intervals = 0;
    RunsBufGuard guard;
    size_t bytesRead = skia_rs_region_read_from_memory(
            static_cast<const uint8_t*>(storage), length,
            &kind, &bounds, &ySpans, &intervals, &guard.ptr);
    if (bytesRead == 0) {
        return 0;
    }
    SkRegion tmp;
    if (kind == 0) {
        // empty (already default)
    } else if (kind == 1) {
        tmp.fBounds = bounds;
        tmp.fRunHead = SkRegion_gRectRunHeadPtr;
    } else {
        size_t len = 0;
        const int32_t* data = skia_rs_region_runs_buf_data(guard.ptr, &len);
        tmp.allocateRuns(SkToInt(len), ySpans, intervals);
        memcpy(tmp.fRunHead->writable_runs(), data, len * sizeof(RunType));
        tmp.fBounds = bounds;
    }
    SkASSERT(tmp.isValid());
    this->swap(tmp);
    return bytesRead;
}

bool SkRegion::isValid() const {
    if (this->isEmpty()) {
        return fBounds == SkIRect{0, 0, 0, 0};
    }
    if (fBounds.isEmpty()) {
        return false;
    }
    if (this->isRect()) {
        return true;
    }
    return fRunHead && fRunHead->fRefCnt > 0 &&
           skia_rs_region_validate_runs(
                   fRunHead->readonly_runs(),
                   static_cast<size_t>(fRunHead->fRunCount),
                   &fBounds,
                   fRunHead->getYSpanCount(),
                   fRunHead->getIntervalCount());
}

#ifdef SK_DEBUG
void SkRegionPriv::Validate(const SkRegion& rgn) {
    SkASSERT(rgn.isValid());
}

void SkRegion::dump() const {
    if (this->isEmpty()) {
        SkDebugf("  rgn: empty\n");
    } else {
        SkDebugf("  rgn: [%d %d %d %d]", fBounds.fLeft, fBounds.fTop, fBounds.fRight, fBounds.fBottom);
        if (this->isComplex()) {
            const RunType* runs = fRunHead->readonly_runs();
            for (int i = 0; i < fRunHead->fRunCount; i++) {
                SkDebugf(" %d", runs[i]);
            }
        }
        SkDebugf("\n");
    }
}
#endif

// ===========================================================================
// SkRegion::Iterator
// ===========================================================================

SkRegion::Iterator::Iterator(const SkRegion& rgn) {
    this->reset(rgn);
}

bool SkRegion::Iterator::rewind() {
    if (fRgn) {
        this->reset(*fRgn);
        return true;
    }
    return false;
}

void SkRegion::Iterator::reset(const SkRegion& rgn) {
    fRgn = &rgn;
    if (rgn.isEmpty()) {
        fDone = true;
    } else {
        fDone = false;
        if (rgn.isRect()) {
            fRect = rgn.fBounds;
            fRuns = nullptr;
        } else {
            fRuns = rgn.fRunHead->readonly_runs();
            fRect.setLTRB(fRuns[3], fRuns[0], fRuns[4], fRuns[1]);
            fRuns += 5;
        }
    }
}

void SkRegion::Iterator::next() {
    if (fDone) {
        return;
    }
    if (fRuns == nullptr) {
        fDone = true;
        return;
    }
    const RunType* base = fRgn->fRunHead->readonly_runs();
    const size_t total = static_cast<size_t>(fRgn->fRunHead->fRunCount);
    const size_t curOff = static_cast<size_t>(fRuns - base);
    size_t nextOff = 0;
    if (skia_rs_region_iter_next(base, total, curOff, &fRect, &nextOff)) {
        fRuns = base + nextOff;
    } else {
        fDone = true;
    }
}

// ===========================================================================
// SkRegion::Cliperator
// ===========================================================================

SkRegion::Cliperator::Cliperator(const SkRegion& rgn, const SkIRect& clip)
        : fIter(rgn), fClip(clip), fDone(true) {
    const SkIRect& r = fIter.rect();
    while (!fIter.done()) {
        if (r.fTop >= clip.fBottom) {
            break;
        }
        if (fRect.intersect(clip, r)) {
            fDone = false;
            break;
        }
        fIter.next();
    }
}

void SkRegion::Cliperator::next() {
    if (fDone) {
        return;
    }
    const SkIRect& r = fIter.rect();
    fDone = true;
    fIter.next();
    while (!fIter.done()) {
        if (r.fTop >= fClip.fBottom) {
            break;
        }
        if (fRect.intersect(fClip, r)) {
            fDone = false;
            break;
        }
        fIter.next();
    }
}

// ===========================================================================
// SkRegion::Spanerator
// ===========================================================================

SkRegion::Spanerator::Spanerator(const SkRegion& rgn, int y, int left, int right) {
    SkDEBUGCODE(SkRegionPriv::Validate(rgn));
    const SkIRect& r = rgn.getBounds();

    fDone = true;
    if (!rgn.isEmpty() && y >= r.fTop && y < r.fBottom &&
            right > r.fLeft && left < r.fRight) {
        if (rgn.isRect()) {
            if (left < r.fLeft) {
                left = r.fLeft;
            }
            if (right > r.fRight) {
                right = r.fRight;
            }
            fLeft = left;
            fRight = right;
            fRuns = nullptr;
            fDone = false;
        } else {
            const RunType* base = rgn.fRunHead->readonly_runs();
            size_t off = 0;
            if (skia_rs_region_spanerator_init(
                        base, static_cast<size_t>(rgn.fRunHead->fRunCount),
                        y, left, right, &off)) {
                fRuns = base + off;
                fLeft = left;
                fRight = right;
                fDone = false;
            }
        }
    }
}

bool SkRegion::Spanerator::next(int* left, int* right) {
    if (fDone) {
        return false;
    }
    if (fRuns == nullptr) {
        fDone = true;
        if (left) {
            *left = fLeft;
        }
        if (right) {
            *right = fRight;
        }
        return true;
    }
    const RunType* runs = fRuns;
    if (runs[0] >= fRight) {
        fDone = true;
        return false;
    }
    SkASSERT(runs[1] > fLeft);
    if (left) {
        *left = std::max(fLeft, runs[0]);
    }
    if (right) {
        *right = std::min(fRight, runs[1]);
    }
    fRuns = runs + 2;
    return true;
}

// ===========================================================================
// SkRegionPriv::VisitSpans
// ===========================================================================

void SkRegionPriv::VisitSpans(const SkRegion& rgn,
                              const std::function<void(const SkIRect&)>& visitor) {
    if (rgn.isEmpty()) {
        return;
    }
    if (rgn.isRect()) {
        visitor(rgn.getBounds());
        return;
    }
    auto cb = [](const SkIRect* r, void* ctx) {
        (*static_cast<const std::function<void(const SkIRect&)>*>(ctx))(*r);
    };
    skia_rs_region_visit_spans(
            rgn.fRunHead->readonly_runs(),
            static_cast<size_t>(rgn.fRunHead->fRunCount),
            cb,
            const_cast<void*>(static_cast<const void*>(&visitor)));
}
