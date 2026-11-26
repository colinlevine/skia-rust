/*
 * Safe Rust bridge for src/core/SkAAClip.cpp (Layer 2.3 skia-rs-region).
 */

#include "src/core/SkAAClip.h"

#include "include/core/SkClipOp.h"
#include "include/core/SkPath.h"
#include "include/core/SkRegion.h"
#include "include/core/SkTypes.h"
#include "include/private/SkCPUTypes.h"
#include "include/private/SkDebug.h"
#include "include/private/SkMacros.h"
#include "include/private/SkMalloc.h"
#include "include/private/SkMath.h"
#include "include/private/SkOnlyStackAllocatable.h"
#include "include/private/SkTDArray.h"
#include "include/private/SkTo.h"
#include "src/core/SkBlitter.h"
#include "src/core/SkColorData.h"
#include "src/core/SkMask.h"
#include "src/core/SkPathPriv.h"
#include "src/core/SkScan.h"

#include <algorithm>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <cstring>

static_assert(sizeof(SkIRect) == 16);

namespace {

struct YOffset {
    int32_t fY;
    uint32_t fOffset;
};
static_assert(sizeof(YOffset) == 8);

#ifdef SK_DEBUG
class AutoAAClipValidate {
public:
    SK_ONLY_STACK_ALLOCATABLE;
    AutoAAClipValidate(const SkAAClip& clip) : fClip(clip) { fClip.validate(); }
    ~AutoAAClipValidate() { fClip.validate(); }
private:
    const SkAAClip& fClip;
};
#define AUTO_AACLIP_VALIDATE(clip) AutoAAClipValidate acv(clip)
#else
#define AUTO_AACLIP_VALIDATE(clip)
#endif

}  // namespace

extern "C" {
struct SkiaRsAAClipBuf;
struct SkiaRsAAClipBuilder;

bool skia_rs_aaclip_buf_info(
        const SkiaRsAAClipBuf* buf, SkIRect* out_bounds, const YOffset** out_yoffsets,
        size_t* out_row_count, const uint8_t** out_data, size_t* out_data_len);
void skia_rs_aaclip_buf_free(SkiaRsAAClipBuf* buf);

size_t skia_rs_aaclip_compute_row_size_for_width(int32_t width);
bool skia_rs_aaclip_validate(
        const SkIRect* bounds, const YOffset* yoffsets, size_t row_count,
        const uint8_t* data, size_t data_len);
bool skia_rs_aaclip_is_rect(
        const SkIRect* bounds, const YOffset* yoffsets, size_t row_count,
        const uint8_t* data, size_t data_len);
bool skia_rs_aaclip_find_row(
        const SkIRect* bounds, const YOffset* yoffsets, size_t row_count, int32_t y,
        size_t* out_offset, int32_t* out_last_y);
void skia_rs_aaclip_find_x(
        const SkIRect* bounds, const uint8_t* row_data, size_t row_len, int32_t x,
        size_t* out_offset, int32_t* out_initial_count);
bool skia_rs_aaclip_quick_contains(
        const SkIRect* bounds, const YOffset* yoffsets, size_t row_count,
        const uint8_t* data, size_t data_len,
        int32_t left, int32_t top, int32_t right, int32_t bottom);
bool skia_rs_aaclip_trim_top_bottom(
        SkIRect* bounds, YOffset* yoffsets, int32_t* row_count,
        const uint8_t* data, size_t data_len);
bool skia_rs_aaclip_trim_left_right(
        SkIRect* bounds, YOffset* yoffsets, size_t row_count,
        uint8_t* data, size_t data_len);
SkiaRsAAClipBuf* skia_rs_aaclip_from_region_rects(
        const SkIRect* bounds, const SkIRect* rects, size_t rect_count);
SkiaRsAAClipBuf* skia_rs_aaclip_operate(
        const SkIRect* target_bounds,
        const SkIRect* a_bounds, const YOffset* a_yoff, size_t a_rows,
        const uint8_t* a_data, size_t a_data_len,
        const SkIRect* b_bounds, const YOffset* b_yoff, size_t b_rows,
        const uint8_t* b_data, size_t b_data_len,
        int32_t op);
void skia_rs_aaclip_expand_to_mask(
        const SkIRect* bounds, const YOffset* yoffsets, size_t row_count,
        const uint8_t* data, size_t data_len, uint8_t* dst, size_t row_bytes);

SkiaRsAAClipBuilder* skia_rs_aaclip_builder_new(const SkIRect* bounds);
void skia_rs_aaclip_builder_free(SkiaRsAAClipBuilder* b);
void skia_rs_aaclip_builder_blit_h(SkiaRsAAClipBuilder* b, int32_t x, int32_t y, int32_t width);
void skia_rs_aaclip_builder_blit_v(
        SkiaRsAAClipBuilder* b, int32_t x, int32_t y, int32_t height, uint8_t alpha);
void skia_rs_aaclip_builder_blit_rect(
        SkiaRsAAClipBuilder* b, int32_t x, int32_t y, int32_t width, int32_t height);
void skia_rs_aaclip_builder_blit_anti_rect(
        SkiaRsAAClipBuilder* b, int32_t x, int32_t y, int32_t width, int32_t height,
        uint8_t left_alpha, uint8_t right_alpha);
void skia_rs_aaclip_builder_blit_anti_h(
        SkiaRsAAClipBuilder* b, int32_t x, int32_t y, const uint8_t* alphas, const int16_t* runs);
SkiaRsAAClipBuf* skia_rs_aaclip_builder_finish(SkiaRsAAClipBuilder* b);

void skia_rs_aaclip_expand_to_runs(
        const uint8_t* row, size_t row_len, int32_t initial_count, int32_t width,
        int16_t* runs, uint8_t* aa);
void skia_rs_aaclip_merge_anti_h(
        const uint8_t* row, size_t row_len, int32_t row_n,
        const uint8_t* src_aa, const int16_t* src_runs,
        uint8_t* dst_aa, int16_t* dst_runs, int32_t max_width);
void skia_rs_aaclip_merge_mask_row_u8(
        const uint8_t* src, int32_t width, const uint8_t* row, size_t row_len,
        int32_t row_n, uint8_t* dst);
void skia_rs_aaclip_merge_mask_row_u16(
        const uint16_t* src, int32_t width, const uint8_t* row, size_t row_len,
        int32_t row_n, uint16_t* dst);
void skia_rs_aaclip_upscale_bw_to_a8(
        const uint8_t* src, size_t src_rb, uint8_t* dst, size_t dst_rb,
        int32_t width, int32_t height);
}  // extern "C"

struct SkAAClip::RunHead {
    std::atomic<int32_t> fRefCnt;
    int32_t fRowCount;
    size_t  fDataSize;

    YOffset* yoffsets() {
        return reinterpret_cast<YOffset*>(reinterpret_cast<char*>(this) + sizeof(RunHead));
    }
    const YOffset* yoffsets() const {
        return reinterpret_cast<const YOffset*>(reinterpret_cast<const char*>(this) + sizeof(RunHead));
    }
    uint8_t* data() {
        return reinterpret_cast<uint8_t*>(this->yoffsets() + fRowCount);
    }
    const uint8_t* data() const {
        return reinterpret_cast<const uint8_t*>(this->yoffsets() + fRowCount);
    }

    static RunHead* Alloc(int rowCount, size_t dataSize) {
        size_t size = sizeof(RunHead) + static_cast<size_t>(rowCount) * sizeof(YOffset) + dataSize;
        RunHead* head = static_cast<RunHead*>(sk_malloc_throw(size));
        head->fRefCnt.store(1);
        head->fRowCount = rowCount;
        head->fDataSize = dataSize;
        return head;
    }

    static RunHead* AllocRect(const SkIRect& bounds) {
        SkASSERT(!bounds.isEmpty());
        int width = bounds.width();
        size_t rowSize = skia_rs_aaclip_compute_row_size_for_width(width);
        RunHead* head = RunHead::Alloc(1, rowSize);
        YOffset* yoff = head->yoffsets();
        yoff->fY = bounds.height() - 1;
        yoff->fOffset = 0;
        uint8_t* row = head->data();
        while (width > 0) {
            int n = std::min(width, 255);
            row[0] = static_cast<uint8_t>(n);
            row[1] = 0xFF;
            width -= n;
            row += 2;
        }
        return head;
    }
};

class SkAAClip::Builder {
public:
    SK_ONLY_STACK_ALLOCATABLE;

    class Blitter final : public SkBlitter {
    public:
        SK_ONLY_STACK_ALLOCATABLE;
        explicit Blitter(SkiaRsAAClipBuilder* b) : fBuilder(b) {}

        void blitV(int x, int y, int height, SkAlpha alpha) override {
            skia_rs_aaclip_builder_blit_v(fBuilder, x, y, height, alpha);
        }
        void blitRect(int x, int y, int width, int height) override {
            skia_rs_aaclip_builder_blit_rect(fBuilder, x, y, width, height);
        }
        void blitAntiRect(int x, int y, int width, int height,
                          SkAlpha leftAlpha, SkAlpha rightAlpha) override {
            skia_rs_aaclip_builder_blit_anti_rect(fBuilder, x, y, width, height, leftAlpha, rightAlpha);
        }
        void blitMask(const SkMask&, const SkIRect&) override {
            SK_ABORT("---- did not expect to get called here");
        }
        void blitH(int x, int y, int width) override {
            skia_rs_aaclip_builder_blit_h(fBuilder, x, y, width);
        }
        void blitAntiH(int x, int y, const SkAlpha alpha[], const int16_t runs[]) override {
            skia_rs_aaclip_builder_blit_anti_h(fBuilder, x, y, alpha, runs);
        }

    private:
        SkiaRsAAClipBuilder* fBuilder;
    };

    static bool InstallBuf(SkAAClip* target, SkiaRsAAClipBuf* buf) {
        SkIRect bounds;
        const YOffset* yoffs = nullptr;
        size_t rowCount = 0;
        const uint8_t* data = nullptr;
        size_t dataLen = 0;
        bool nonEmpty = skia_rs_aaclip_buf_info(
                buf, &bounds, &yoffs, &rowCount, &data, &dataLen);
        if (!nonEmpty || rowCount == 0) {
            skia_rs_aaclip_buf_free(buf);
            return target->setEmpty();
        }
        RunHead* head = RunHead::Alloc(SkToInt(rowCount), dataLen);
        memcpy(head->yoffsets(), yoffs, rowCount * sizeof(YOffset));
        memcpy(head->data(), data, dataLen);
        skia_rs_aaclip_buf_free(buf);

        target->setEmpty();
        target->fBounds = bounds;
        target->fRunHead = head;
        target->validate();
        return true;
    }

    static bool BlitPath(SkAAClip* target, const SkIRect& bounds, const SkPath& path, bool doAA) {
        SkiaRsAAClipBuilder* b = skia_rs_aaclip_builder_new(&bounds);
        Blitter blitter(b);
        SkRegion clip(bounds);

        const auto rc = SkResolveConvexity::kYes;
        SkPathRaw raw = SkPathPriv::Raw(path, rc).value_or(SkPathRaw::Empty(path.getFillType()));
        if (doAA) {
            SkScan::AntiFillPath(raw, clip, &blitter, true);
        } else {
            SkScan::FillPath(raw, clip, &blitter);
        }
        SkiaRsAAClipBuf* buf = skia_rs_aaclip_builder_finish(b);
        return InstallBuf(target, buf);
    }
};

// ===========================================================================
// SkAAClip
// ===========================================================================

SkAAClip::SkAAClip() {
    fBounds.setEmpty();
    fRunHead = nullptr;
}

SkAAClip::SkAAClip(const SkAAClip& src) {
    SkDEBUGCODE(fBounds.setEmpty();)
    fRunHead = nullptr;
    *this = src;
}

SkAAClip::~SkAAClip() {
    this->freeRuns();
}

SkAAClip& SkAAClip::operator=(const SkAAClip& src) {
    AUTO_AACLIP_VALIDATE(*this);
    src.validate();

    if (this != &src) {
        this->freeRuns();
        fBounds = src.fBounds;
        fRunHead = src.fRunHead;
        if (fRunHead) {
            fRunHead->fRefCnt++;
        }
    }
    return *this;
}

bool SkAAClip::setEmpty() {
    this->freeRuns();
    fBounds.setEmpty();
    fRunHead = nullptr;
    return false;
}

bool SkAAClip::setRect(const SkIRect& bounds) {
    if (bounds.isEmpty()) {
        return this->setEmpty();
    }
    AUTO_AACLIP_VALIDATE(*this);
    this->freeRuns();
    fBounds = bounds;
    fRunHead = RunHead::AllocRect(bounds);
    return true;
}

bool SkAAClip::isRect() const {
    if (this->isEmpty()) {
        return false;
    }
    const RunHead* head = fRunHead;
    return skia_rs_aaclip_is_rect(
            &fBounds, head->yoffsets(), static_cast<size_t>(head->fRowCount),
            head->data(), head->fDataSize);
}

bool SkAAClip::setRegion(const SkRegion& rgn) {
    if (rgn.isEmpty()) {
        return this->setEmpty();
    }
    if (rgn.isRect()) {
        return this->setRect(rgn.getBounds());
    }

    const SkIRect& bounds = rgn.getBounds();
    SkTDArray<SkIRect> rects;
    for (SkRegion::Iterator iter(rgn); !iter.done(); iter.next()) {
        rects.push_back(iter.rect());
    }
    SkiaRsAAClipBuf* buf = skia_rs_aaclip_from_region_rects(
            &bounds, rects.begin(), static_cast<size_t>(rects.size()));
    return Builder::InstallBuf(this, buf);
}

bool SkAAClip::setPath(const SkPath& path, const SkIRect& clip, bool doAA) {
    AUTO_AACLIP_VALIDATE(*this);

    if (clip.isEmpty()) {
        return this->setEmpty();
    }

    SkIRect ibounds;
    if (path.isInverseFillType()) {
        ibounds = clip;
    } else {
        path.getBounds().roundOut(&ibounds);
        if (ibounds.isEmpty64() || !ibounds.intersect(clip)) {
            return this->setEmpty();
        }
    }

    return Builder::BlitPath(this, ibounds, path, doAA);
}

bool SkAAClip::op(const SkAAClip& other, SkClipOp op) {
    AUTO_AACLIP_VALIDATE(*this);

    if (this->isEmpty()) {
        return false;
    }

    SkIRect bounds = fBounds;
    switch (op) {
        case SkClipOp::kDifference:
            if (other.isEmpty() || !SkIRect::Intersects(fBounds, other.fBounds)) {
                return true;
            }
            break;
        case SkClipOp::kIntersect:
            if (other.isEmpty() || !bounds.intersect(other.fBounds)) {
                return this->setEmpty();
            }
            break;
    }

    const RunHead* aHead = fRunHead;
    const RunHead* bHead = other.fRunHead;
    SkiaRsAAClipBuf* buf = skia_rs_aaclip_operate(
            &bounds,
            &fBounds, aHead->yoffsets(), static_cast<size_t>(aHead->fRowCount),
            aHead->data(), aHead->fDataSize,
            &other.fBounds, bHead->yoffsets(), static_cast<size_t>(bHead->fRowCount),
            bHead->data(), bHead->fDataSize,
            static_cast<int32_t>(op));
    return Builder::InstallBuf(this, buf);
}

bool SkAAClip::op(const SkIRect& rect, SkClipOp op) {
    SkIRect pixelBounds = fBounds;
    if (!pixelBounds.intersect(rect)) {
        switch (op) {
            case SkClipOp::kDifference: return !this->isEmpty();
            case SkClipOp::kIntersect:  return this->setEmpty();
        }
        SkUNREACHABLE;
    } else if (pixelBounds == fBounds) {
        switch (op) {
            case SkClipOp::kDifference: return this->setEmpty();
            case SkClipOp::kIntersect:  return !this->isEmpty();
        }
        SkUNREACHABLE;
    } else if (op == SkClipOp::kIntersect && this->quickContains(pixelBounds)) {
        return this->setRect(pixelBounds);
    } else {
        SkAAClip clip;
        clip.setRect(rect);
        return this->op(clip, op);
    }
}

bool SkAAClip::op(const SkRect& rect, SkClipOp op, bool doAA) {
    if (!doAA) {
        return this->op(rect.round(), op);
    } else {
        SkIRect pixelBounds = fBounds;
        if (!pixelBounds.intersect(rect.roundOut())) {
            switch (op) {
                case SkClipOp::kDifference: return !this->isEmpty();
                case SkClipOp::kIntersect:  return this->setEmpty();
            }
            SkUNREACHABLE;
        } else if (rect.contains(SkRect::Make(fBounds))) {
            switch (op) {
                case SkClipOp::kDifference: return this->setEmpty();
                case SkClipOp::kIntersect:  return !this->isEmpty();
            }
            SkUNREACHABLE;
        } else if (op == SkClipOp::kIntersect && this->quickContains(pixelBounds)) {
            return this->setPath(SkPath::Rect(rect), pixelBounds, /*doAA=*/true);
        } else {
            SkAAClip rectClip;
            rectClip.setPath(SkPath::Rect(rect),
                             op == SkClipOp::kDifference ? fBounds : pixelBounds,
                             /*doAA=*/true);
            return this->op(rectClip, op);
        }
    }
}

bool SkAAClip::translate(int dx, int dy, SkAAClip* dst) const {
    if (nullptr == dst) {
        return !this->isEmpty();
    }
    if (this->isEmpty()) {
        return dst->setEmpty();
    }
    if (this != dst) {
        fRunHead->fRefCnt++;
        dst->freeRuns();
        dst->fRunHead = fRunHead;
        dst->fBounds = fBounds;
    }
    dst->fBounds.offset(dx, dy);
    return true;
}

void SkAAClip::freeRuns() {
    if (fRunHead) {
        SkASSERT(fRunHead->fRefCnt.load() >= 1);
        if (1 == fRunHead->fRefCnt--) {
            sk_free(fRunHead);
        }
    }
}

void SkAAClip::copyToMask(SkMaskBuilder* mask) const {
    mask->format() = SkMask::kA8_Format;
    if (this->isEmpty()) {
        mask->bounds().setEmpty();
        mask->image() = nullptr;
        mask->rowBytes() = 0;
        return;
    }

    mask->bounds() = fBounds;
    mask->rowBytes() = fBounds.width();
    size_t size = mask->computeImageSize();
    mask->image() = SkMaskBuilder::AllocImage(size);

    const RunHead* head = fRunHead;
    skia_rs_aaclip_expand_to_mask(
            &fBounds, head->yoffsets(), static_cast<size_t>(head->fRowCount),
            head->data(), head->fDataSize, mask->image(), mask->fRowBytes);
}

const uint8_t* SkAAClip::findRow(int y, int* lastYForRow) const {
    SkASSERT(fRunHead);
    size_t offset = 0;
    int32_t lastY = 0;
    if (!skia_rs_aaclip_find_row(
                &fBounds, fRunHead->yoffsets(), static_cast<size_t>(fRunHead->fRowCount),
                y, &offset, &lastY)) {
        return nullptr;
    }
    if (lastYForRow) {
        *lastYForRow = lastY;
    }
    return fRunHead->data() + offset;
}

const uint8_t* SkAAClip::findX(const uint8_t data[], int x, int* initialCount) const {
    SkASSERT(x >= fBounds.fLeft && x < fBounds.fRight);
    const uint8_t* end = fRunHead->data() + fRunHead->fDataSize;
    size_t rowLen = static_cast<size_t>(end - data);
    size_t offset = 0;
    int32_t initCount = 0;
    skia_rs_aaclip_find_x(&fBounds, data, rowLen, x, &offset, &initCount);
    if (initialCount) {
        *initialCount = initCount;
    }
    return data + offset;
}

bool SkAAClip::quickContains(int left, int top, int right, int bottom) const {
    if (this->isEmpty()) {
        return false;
    }
    const RunHead* head = fRunHead;
    return skia_rs_aaclip_quick_contains(
            &fBounds, head->yoffsets(), static_cast<size_t>(head->fRowCount),
            head->data(), head->fDataSize, left, top, right, bottom);
}

bool SkAAClip::trimTopBottom() {
    if (this->isEmpty()) {
        return false;
    }
    this->validate();
    RunHead* head = fRunHead;
    const int32_t oldRowCount = head->fRowCount;
    const uint8_t* oldData = head->data();
    int32_t rowCount = oldRowCount;
    if (!skia_rs_aaclip_trim_top_bottom(
                &fBounds, head->yoffsets(), &rowCount, oldData, head->fDataSize)) {
        return this->setEmpty();
    }
    if (rowCount < oldRowCount) {
        memmove(head->yoffsets() + rowCount, oldData, head->fDataSize);
        head->fRowCount = rowCount;
    }
    this->validate();
    return true;
}

bool SkAAClip::trimLeftRight() {
    if (this->isEmpty()) {
        return false;
    }
    AUTO_AACLIP_VALIDATE(*this);
    RunHead* head = fRunHead;
    if (!skia_rs_aaclip_trim_left_right(
                &fBounds, head->yoffsets(), static_cast<size_t>(head->fRowCount),
                head->data(), head->fDataSize)) {
        return this->setEmpty();
    }
    return true;
}

bool SkAAClip::trimBounds() {
    if (this->isEmpty()) {
        return false;
    }
    const RunHead* head = fRunHead;
    const YOffset* yoff = head->yoffsets();
    SkASSERT(head->fRowCount > 0);
    const YOffset& lastY = yoff[head->fRowCount - 1];
    SkASSERT(lastY.fY + 1 <= fBounds.height());
    fBounds.fBottom = fBounds.fTop + lastY.fY + 1;
    SkASSERT(!fBounds.isEmpty());
    return this->trimTopBottom() && this->trimLeftRight();
}

#ifdef SK_DEBUG
void SkAAClip::validate() const {
    if (nullptr == fRunHead) {
        SkASSERT(fBounds.isEmpty());
        return;
    }
    SkASSERT(skia_rs_aaclip_validate(
            &fBounds, fRunHead->yoffsets(), static_cast<size_t>(fRunHead->fRowCount),
            fRunHead->data(), fRunHead->fDataSize));
}

void SkAAClip::debug(bool compress_y) const {
    if (this->isEmpty()) {
        return;
    }
    const RunHead* head = fRunHead;
    const int width = fBounds.width();
    const YOffset* yoffs = head->yoffsets();
    int top = fBounds.fTop;
    for (int r = 0; r < head->fRowCount; ++r) {
        int bot = fBounds.fTop + yoffs[r].fY + 1;
        const uint8_t* row = head->data() + yoffs[r].fOffset;
        auto dumpRow = [&](int repeat) {
            if (repeat) {
                SkDebugf("*%d ", repeat);
            }
            int w = width;
            const uint8_t* p = row;
            while (w > 0) {
                int n = p[0];
                int val = p[1];
                for (int i = 0; i < n; ++i) {
                    SkDebugf("%02X ", val);
                }
                p += 2;
                w -= n;
            }
            SkDebugf("\n");
        };
        if (compress_y) {
            dumpRow(bot - top + 1);
        } else {
            for (int y = top; y < bot; ++y) {
                dumpRow(0);
            }
        }
        top = bot;
    }
}
#endif

// ===========================================================================
// SkAAClipBlitter
// ===========================================================================

SkAAClipBlitter::~SkAAClipBlitter() = default;

void SkAAClipBlitter::ensureRunsAndAA() {
    if (nullptr == fScanlineScratch) {
        int count = fAAClipBounds.width() + 1;
        fScanlineScratch.reset(sk_malloc_throw(static_cast<size_t>(count) * sizeof(SkPMColor)));
        fRuns = static_cast<int16_t*>(fScanlineScratch.get());
        fAA = reinterpret_cast<SkAlpha*>(fRuns + count);
    }
}

void SkAAClipBlitter::blitH(int x, int y, int width) {
    SkASSERT(width > 0);
    SkASSERT(fAAClipBounds.contains(x, y));
    SkASSERT(fAAClipBounds.contains(x + width - 1, y));

    const uint8_t* row = fAAClip->findRow(y);
    int initialCount;
    row = fAAClip->findX(row, x, &initialCount);

    if (initialCount >= width) {
        SkAlpha alpha = row[1];
        if (0 == alpha) {
            return;
        }
        if (0xFF == alpha) {
            fBlitter->blitH(x, y, width);
            return;
        }
    }

    this->ensureRunsAndAA();
    const uint8_t* end = fAAClip->fRunHead->data() + fAAClip->fRunHead->fDataSize;
    skia_rs_aaclip_expand_to_runs(
            row, static_cast<size_t>(end - row), initialCount, width, fRuns, fAA);
    fBlitter->blitAntiH(x, y, fAA, fRuns);
}

void SkAAClipBlitter::blitAntiH(int x, int y, const SkAlpha aa[], const int16_t runs[]) {
    const uint8_t* row = fAAClip->findRow(y);
    int initialCount;
    row = fAAClip->findX(row, x, &initialCount);

    this->ensureRunsAndAA();
    const uint8_t* end = fAAClip->fRunHead->data() + fAAClip->fRunHead->fDataSize;
    skia_rs_aaclip_merge_anti_h(
            row, static_cast<size_t>(end - row), initialCount,
            aa, runs, fAA, fRuns, fAAClipBounds.width());
    fBlitter->blitAntiH(x, y, fAA, fRuns);
}

void SkAAClipBlitter::blitV(int x, int y, int height, SkAlpha alpha) {
    if (fAAClip->quickContains(x, y, x + 1, y + height)) {
        fBlitter->blitV(x, y, height, alpha);
        return;
    }

    for (;;) {
        int lastY SK_INIT_TO_AVOID_WARNING;
        const uint8_t* row = fAAClip->findRow(y, &lastY);
        int dy = lastY - y + 1;
        if (dy > height) {
            dy = height;
        }
        height -= dy;

        row = fAAClip->findX(row, x);
        SkAlpha newAlpha = SkMulDiv255Round(alpha, row[1]);
        if (newAlpha) {
            fBlitter->blitV(x, y, dy, newAlpha);
        }
        SkASSERT(height >= 0);
        if (height <= 0) {
            break;
        }
        y = lastY + 1;
    }
}

void SkAAClipBlitter::blitRect(int x, int y, int width, int height) {
    if (fAAClip->quickContains(x, y, x + width, y + height)) {
        fBlitter->blitRect(x, y, width, height);
        return;
    }
    while (--height >= 0) {
        this->blitH(x, y, width);
        y += 1;
    }
}

void SkAAClipBlitter::blitMask(const SkMask& origMask, const SkIRect& clip) {
    SkASSERT(fAAClip->getBounds().contains(clip));

    if (fAAClip->quickContains(clip)) {
        fBlitter->blitMask(origMask, clip);
        return;
    }

    const SkMask* mask = &origMask;

    SkMaskBuilder grayMask;
    if (SkMask::kBW_Format == origMask.fFormat) {
        grayMask.format() = SkMask::kA8_Format;
        grayMask.bounds() = origMask.fBounds;
        grayMask.rowBytes() = origMask.fBounds.width();
        size_t size = grayMask.computeImageSize();
        grayMask.image() = reinterpret_cast<uint8_t*>(
                fGrayMaskScratch.reset(size, SkAutoMalloc::kReuse_OnShrink));

        skia_rs_aaclip_upscale_bw_to_a8(
                origMask.fImage, origMask.fRowBytes,
                grayMask.image(), grayMask.fRowBytes,
                origMask.fBounds.width(), origMask.fBounds.height());
        mask = &grayMask;
    }

    this->ensureRunsAndAA();

    const void* src = mask->getAddr(clip.fLeft, clip.fTop);
    const size_t srcRB = mask->fRowBytes;
    const int width = clip.width();
    const bool isLCD16 = (mask->fFormat == SkMask::kLCD16_Format);
    const uint8_t* dataEnd = fAAClip->fRunHead->data() + fAAClip->fRunHead->fDataSize;

    SkMaskBuilder rowMask;
    rowMask.format() = SkMask::k3D_Format == mask->fFormat ? SkMask::kA8_Format : mask->fFormat;
    rowMask.bounds().fLeft = clip.fLeft;
    rowMask.bounds().fRight = clip.fRight;
    rowMask.rowBytes() = mask->fRowBytes;
    rowMask.image() = static_cast<uint8_t*>(fScanlineScratch.get());

    int y = clip.fTop;
    const int stopY = y + clip.height();

    do {
        int localStopY SK_INIT_TO_AVOID_WARNING;
        const uint8_t* row = fAAClip->findRow(y, &localStopY);
        localStopY = std::min(localStopY + 1, stopY);

        int initialCount;
        row = fAAClip->findX(row, clip.fLeft, &initialCount);
        const size_t rowLen = static_cast<size_t>(dataEnd - row);
        do {
            if (isLCD16) {
                skia_rs_aaclip_merge_mask_row_u16(
                        static_cast<const uint16_t*>(src), width, row, rowLen, initialCount,
                        reinterpret_cast<uint16_t*>(rowMask.image()));
            } else {
                skia_rs_aaclip_merge_mask_row_u8(
                        static_cast<const uint8_t*>(src), width, row, rowLen, initialCount,
                        rowMask.image());
            }
            rowMask.bounds().fTop = y;
            rowMask.bounds().fBottom = y + 1;
            fBlitter->blitMask(rowMask, rowMask.fBounds);
            src = static_cast<const void*>(static_cast<const char*>(src) + srcRB);
        } while (++y < localStopY);
    } while (y < stopY);
}
