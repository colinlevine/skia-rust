/*
 * Safe Rust bridge for:
 *   src/utils/SkMultiPictureDocument.cpp
 * (Phase 1 Layer 5.2 skia-rs-picture, GN target :skia).
 */

#include "include/core/SkCanvas.h"
#include "include/core/SkData.h"
#include "include/core/SkDocument.h"
#include "include/core/SkPicture.h"
#include "include/core/SkPictureRecorder.h"
#include "include/core/SkRect.h"
#include "include/core/SkScalar.h"
#include "include/core/SkSerialProcs.h"
#include "include/core/SkSize.h"
#include "include/core/SkStream.h"
#include "include/docs/SkMultiPictureDocument.h"
#include "include/private/SkDebug.h"
#include "include/private/SkOnlyStackAllocatable.h"
#include "include/private/SkTArray.h"
#include "include/private/SkTemplates.h"
#include "include/private/SkTo.h"
#include "include/utils/SkNWayCanvas.h"
#include "src/utils/SkMultiPictureDocumentPriv.h"

#include <cstddef>
#include <cstdint>
#include <functional>
#include <utility>

using namespace skia_private;

static_assert(sizeof(SkSize) == 8);

extern "C" {
SkSize skia_rs_mpd_join_page_sizes(const SkSize* sizes, size_t count);
size_t skia_rs_mpd_encode_header(
        const SkSize* sizes, size_t count, uint8_t* out_buf, size_t out_cap);
int32_t skia_rs_mpd_parse_header_prefix(const uint8_t* header, size_t len);
bool skia_rs_mpd_validate_page_count_and_length(
        int32_t page_count, int32_t dst_array_count, bool has_length, size_t stream_length);
bool skia_rs_mpd_is_valid_page_size(SkSize size);
const char* skia_rs_mpd_end_page_key();
bool skia_rs_mpd_is_end_page_annotation(const char* key);
}  // extern "C"

namespace {

static constexpr size_t kMpdHeaderPrefixSize = 32;

struct MultiPictureDocument final : public SkDocument {
    const SkSerialProcs fProcs;
    SkPictureRecorder fPictureRecorder;
    SkSize fCurrentPageSize;
    TArray<sk_sp<SkPicture>> fPages;
    TArray<SkSize> fSizes;
    std::function<void(const SkPicture*)> fOnEndPage;

    MultiPictureDocument(SkWStream* s,
                         const SkSerialProcs* procs,
                         std::function<void(const SkPicture*)> onEndPage)
            : SkDocument(s)
            , fProcs(procs ? *procs : SkSerialProcs())
            , fOnEndPage(std::move(onEndPage)) {}

    ~MultiPictureDocument() override { this->close(); }

    SkCanvas* onBeginPage(SkScalar w, SkScalar h) override {
        fCurrentPageSize.set(w, h);
        return fPictureRecorder.beginRecording(w, h);
    }

    void onEndPage() override {
        fSizes.push_back(fCurrentPageSize);
        sk_sp<SkPicture> lastPage = fPictureRecorder.finishRecordingAsPicture();
        fPages.push_back(lastPage);
        if (fOnEndPage) {
            fOnEndPage(lastPage.get());
        }
    }

    void onClose(SkWStream* wStream) override {
        SkASSERT(wStream);
        SkASSERT(wStream->bytesWritten() == 0);
        const size_t headerSize = skia_rs_mpd_encode_header(
                fSizes.begin(), SkToSizeT(fSizes.size()), nullptr, 0);
        AutoSTMalloc<128, uint8_t> headerBuf(headerSize);
        skia_rs_mpd_encode_header(
                fSizes.begin(), SkToSizeT(fSizes.size()), headerBuf.get(), headerSize);
        wStream->write(headerBuf.get(), headerSize);

        SkSize bigsize = skia_rs_mpd_join_page_sizes(fSizes.begin(), SkToSizeT(fSizes.size()));
        SkCanvas* c = fPictureRecorder.beginRecording(SkRect::MakeSize(bigsize));
        const char* endPageKey = skia_rs_mpd_end_page_key();
        for (const sk_sp<SkPicture>& page : fPages) {
            c->drawPicture(page);
            c->drawAnnotation(SkRect::MakeEmpty(), endPageKey, SkData::MakeWithCString("X"));
        }
        sk_sp<SkPicture> p = fPictureRecorder.finishRecordingAsPicture();
        p->serialize(wStream, &fProcs);
        fPages.clear();
        fSizes.clear();
    }

    void onAbort() override {
        fPages.clear();
        fSizes.clear();
    }
};

struct PagerCanvas : public SkNWayCanvas {
    SK_ONLY_STACK_ALLOCATABLE;

    SkPictureRecorder fRecorder;
    SkDocumentPage* fDst;
    int fCount;
    int fIndex = 0;

    PagerCanvas(SkISize wh, SkDocumentPage* dst, int count)
            : SkNWayCanvas(wh.width(), wh.height()), fDst(dst), fCount(count) {
        this->nextCanvas();
    }

    void nextCanvas() {
        if (fIndex < fCount) {
            SkRect bounds = SkRect::MakeSize(fDst[fIndex].fSize);
            this->addCanvas(fRecorder.beginRecording(bounds));
        }
    }

    void onDrawAnnotation(const SkRect& r, const char* key, SkData* d) override {
        if (skia_rs_mpd_is_end_page_annotation(key)) {
            this->removeAll();
            if (fIndex < fCount) {
                fDst[fIndex].fPicture = fRecorder.finishRecordingAsPicture();
                ++fIndex;
            }
            this->nextCanvas();
        } else {
            this->SkNWayCanvas::onDrawAnnotation(r, key, d);
        }
    }
};

}  // namespace

namespace SkMultiPictureDocument {

sk_sp<SkDocument> Make(SkWStream* dst,
                       const SkSerialProcs* procs,
                       std::function<void(const SkPicture*)> onEndPage) {
    return sk_make_sp<MultiPictureDocument>(dst, procs, std::move(onEndPage));
}

int ReadPageCount(SkStreamSeekable* src) {
    if (!src) {
        return 0;
    }
    src->seek(0);
    uint8_t header[kMpdHeaderPrefixSize];
    if (kMpdHeaderPrefixSize != src->read(header, kMpdHeaderPrefixSize)) {
        return 0;
    }
    int32_t pageCount = skia_rs_mpd_parse_header_prefix(header, kMpdHeaderPrefixSize);
    if (pageCount < 0) {
        return 0;
    }
    return pageCount;
}

bool ReadPageSizes(SkStreamSeekable* src, SkDocumentPage* dstArray, int dstArrayCount) {
    if (!dstArray || dstArrayCount < 1) {
        return false;
    }
    int pageCount = ReadPageCount(src);
    if (!skia_rs_mpd_validate_page_count_and_length(
                pageCount, dstArrayCount, src->hasLength(), src->hasLength() ? src->getLength() : 0)) {
        return false;
    }
    for (int i = 0; i < pageCount; ++i) {
        SkSize& s = dstArray[i].fSize;
        if (sizeof(s) != src->read(&s, sizeof(s))) {
            return false;
        }
        if (!skia_rs_mpd_is_valid_page_size(s)) {
            return false;
        }
    }
    return true;
}

bool Read(SkStreamSeekable* src,
          SkDocumentPage* dstArray,
          int dstArrayCount,
          const SkDeserialProcs* procs) {
    if (!ReadPageSizes(src, dstArray, dstArrayCount)) {
        return false;
    }
    AutoSTArray<16, SkSize> sizes(dstArrayCount);
    for (int i = 0; i < dstArrayCount; ++i) {
        sizes[i] = dstArray[i].fSize;
    }
    SkSize joined = skia_rs_mpd_join_page_sizes(sizes.get(), SkToSizeT(dstArrayCount));

    auto picture = SkPicture::MakeFromStream(src, procs);
    if (!picture) {
        return false;
    }

    PagerCanvas canvas(joined.toCeil(), dstArray, dstArrayCount);
    picture->playback(&canvas);
    if (canvas.fIndex != dstArrayCount) {
        SkDEBUGF("Malformed SkMultiPictureDocument: canvas.fIndex=%d dstArrayCount=%d\n",
                 canvas.fIndex,
                 dstArrayCount);
    }
    return true;
}

}  // namespace SkMultiPictureDocument
