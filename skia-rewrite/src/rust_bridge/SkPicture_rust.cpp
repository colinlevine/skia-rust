/*
 * Safe Rust bridge for:
 *   src/core/SkPicture.cpp
 *   src/core/SkPictureData.cpp
 *   src/core/SkPictureFlat.cpp
 *   src/core/SkPicturePlayback.cpp
 *   src/core/SkPictureRecord.cpp
 *   src/core/SkPictureRecorder.cpp
 *   src/core/SkPtrRecorder.cpp
 * (Phase 1 Layer 5.2 skia-rs-picture, GN target :core).
 */

#include "include/core/SkBBHFactory.h"
#include "include/core/SkBlendMode.h"
#include "include/core/SkCanvas.h"
#include "include/core/SkClipOp.h"
#include "include/core/SkColor.h"
#include "include/core/SkData.h"
#include "include/core/SkDrawable.h"
#include "include/core/SkFlattenable.h"
#include "include/core/SkFontMgr.h"
#include "include/core/SkImage.h"
#include "include/core/SkImageFilter.h"
#include "include/core/SkM44.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPath.h"
#include "include/core/SkPicture.h"
#include "include/core/SkPictureRecorder.h"
#include "include/core/SkPoint.h"
#include "include/core/SkPoint3.h"
#include "include/core/SkRRect.h"
#include "include/core/SkRSXform.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkRegion.h"
#include "include/core/SkSamplingOptions.h"
#include "include/core/SkScalar.h"
#include "include/core/SkSerialProcs.h"
#include "include/core/SkShader.h"
#include "include/core/SkStream.h"
#include "include/core/SkString.h"
#include "include/core/SkSurface.h"
#include "include/core/SkTextBlob.h"
#include "include/core/SkTileMode.h"
#include "include/core/SkTypeface.h"
#include "include/core/SkTypes.h"
#include "include/core/SkVertices.h"
#include "include/private/SkAlign.h"
#include "include/private/SkAssert.h"
#include "include/private/SkDebug.h"
#include "include/private/SkOnlyStackAllocatable.h"
#include "include/private/SkTArray.h"
#include "include/private/SkTFitsIn.h"
#include "include/private/SkTemplates.h"
#include "include/private/SkTo.h"
#include "include/private/chromium/Slug.h"
#include "src/core/SkAutoMalloc.h"
#include "src/core/SkCanvasPriv.h"
#include "src/core/SkDebugUtils.h"
#include "src/core/SkDrawShadowInfo.h"
#include "src/core/SkMathPriv.h"
#include "src/core/SkMatrixPriv.h"
#include "src/core/SkPictureData.h"
#include "src/core/SkPictureFlat.h"
#include "src/core/SkPicturePlayback.h"
#include "src/core/SkPicturePriv.h"
#include "src/core/SkPictureRecord.h"
#include "src/core/SkPtrRecorder.h"
#include "src/core/SkReadBuffer.h"
#include "src/core/SkRecord.h"
#include "src/core/SkRecordCanvas.h"
#include "src/core/SkRecordDraw.h"
#include "src/core/SkRecordOpts.h"
#include "src/core/SkRecordedDrawable.h"
#include "src/core/SkRecords.h"
#include "src/core/SkResourceCache.h"
#include "src/core/SkSafeMath.h"
#include "src/core/SkSamplingPriv.h"
#include "src/core/SkStreamPriv.h"
#include "src/core/SkTHash.h"
#include "src/core/SkTextBlobPriv.h"
#include "src/core/SkVerticesPriv.h"
#include "src/core/SkWriteBuffer.h"
#include "src/utils/SkPatchUtils.h"

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <memory>
#include <utility>

using namespace skia_private;

static_assert(sizeof(SkPictInfo) == 28);
static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkIRect) == 16);
static_assert(sizeof(SkPoint) == 8);
static_assert(sizeof(SkMatrix) == 40);
static_assert(sizeof(SkM44) == 64);

namespace {

struct RsPtrPair {
    size_t ptr;
    uint32_t index;
};

struct RsPtrInsertResult {
    bool found;
    size_t pos;
    uint32_t index;
};

enum class RsOptionalPaintResult : int32_t {
    kNone = 0,
    kValid = 1,
    kInvalid = 2,
};

struct RsUnpackedOpHeader {
    bool valid;
    bool needs_extra_size;
    uint32_t op;
    uint32_t size;
};

struct RsUnpackedClipRegionOp {
    bool valid;
    uint32_t region_op;
};

enum class RsClipOpAction : int32_t {
    kIgnore = 0,
    kDirectDifference = 1,
    kDirectIntersect = 2,
    kResetThenIntersect = 3,
};

struct RsSaveLayerFlatLayout {
    uint32_t flat_flags;
    size_t total_bytes;
};

struct RsDrawPatchFlatLayout {
    uint32_t flags;
    size_t total_bytes;
};

struct RsDrawAtlasFlatLayout {
    uint32_t flags;
    size_t total_bytes;
};

enum {
    kFailure_TrailingStreamByteAfterPictInfo = 0,
    kPictureData_TrailingStreamByteAfterPictInfo = 1,
    kCustom_TrailingStreamByteAfterPictInfo = 2,
};

enum {
    kNoInitialSave = -1,
};

static constexpr int kUInt32Size = 4;

}  // namespace

extern "C" {
// SkPtrSet
bool skia_rs_ptr_set_less(RsPtrPair a, RsPtrPair b);
uint32_t skia_rs_ptr_set_find(const RsPtrPair* pairs, size_t count, size_t target);
RsPtrInsertResult skia_rs_ptr_set_find_or_insert(
        const RsPtrPair* pairs, size_t count, size_t target);
void skia_rs_ptr_set_copy_to_array(const RsPtrPair* pairs, size_t count, size_t* out);

// SkPicture / SkPictInfo / SkPictureData / SkPicturePlayback / SkPictureRecord / SkPictureRecorder
uint32_t skia_rs_picture_next_id();
SkPictInfo skia_rs_picture_create_header(SkRect cull_rect);
bool skia_rs_picture_is_valid_pict_info(const SkPictInfo* info);
void skia_rs_picture_encode_pict_info(const SkPictInfo* info, uint8_t* out_bytes);
bool skia_rs_picture_decode_pict_info(const uint8_t* bytes, SkPictInfo* out_info);
size_t skia_rs_picture_negate_to_size_t(int32_t val);
bool skia_rs_picture_validate_custom_serialize_size(size_t size);
size_t skia_rs_picture_pad32_bytes(size_t size);
bool skia_rs_picture_should_use_bbh(SkRect local_clip, SkRect cull_rect);
size_t skia_rs_picture_approx_bytes_used(
        bool is_placeholder,
        size_t self_size,
        size_t record_bytes,
        size_t sub_pic_bytes,
        size_t bbh_bytes);
SkRect skia_rs_picture_normalize_recording_cull_rect(SkRect user_cull);
SkRect skia_rs_picture_compute_bbh_union(const SkRect* bounds, size_t count);
size_t skia_rs_picture_compute_factory_chunk_size(const size_t* name_lens, size_t count);
RsOptionalPaintResult skia_rs_picture_validate_optional_paint_index(
        int32_t index, int32_t table_len);
RsUnpackedOpHeader skia_rs_picture_unpack_op_word(uint32_t bits);
bool skia_rs_picture_validate_op_and_size(uint32_t op, uint32_t size);
bool skia_rs_picture_validate_offset_to_restore(size_t offset_to_restore, size_t reader_offset);
uint32_t skia_rs_picture_clip_params_pack(uint32_t op, bool do_aa);
bool skia_rs_picture_clip_params_unpack_do_aa(uint32_t packed);
RsUnpackedClipRegionOp skia_rs_picture_clip_params_unpack_region_op(
        uint32_t packed, uint32_t version);
RsClipOpAction skia_rs_picture_map_region_op_to_clip_action(uint32_t region_op);
RsSaveLayerFlatLayout skia_rs_picture_compute_save_layer_flat_layout(
        bool has_bounds,
        bool has_paint,
        bool has_backdrop,
        uint32_t save_layer_flags,
        float backdrop_scale,
        uint32_t filter_count,
        bool backdrop_tile_mode_is_clamp);
RsDrawPatchFlatLayout skia_rs_picture_compute_draw_patch_flat_layout(
        bool has_colors, bool has_tex_coords, bool is_modulate_blend);
RsDrawAtlasFlatLayout skia_rs_picture_compute_draw_atlas_flat_layout(
        size_t count, bool has_colors, bool has_cull, size_t sampling_flat_size);
bool skia_rs_picture_validate_edge_aa_entries_avail(int32_t cnt, size_t available);
bool skia_rs_picture_validate_edge_aa_matrices_avail(
        int32_t matrix_count, int32_t max_matrix_index, size_t available);
bool skia_rs_picture_validate_save_layer_filter_count(int32_t filter_count);
int32_t skia_rs_picture_find_id_linear(const uint64_t* ids, size_t count, uint64_t target);
}  // extern "C"

///////////////////////////////////////////////////////////////////////////////////////////////////
// SkPtrSet (src/core/SkPtrRecorder.cpp)
///////////////////////////////////////////////////////////////////////////////////////////////////

void SkPtrSet::reset() {
    Pair* p = fList.begin();
    Pair* stop = fList.end();
    while (p < stop) {
        this->decPtr(p->fPtr);
        p += 1;
    }
    fList.reset();
}

bool SkPtrSet::Less(const Pair& a, const Pair& b) {
    static_assert(sizeof(Pair) == sizeof(RsPtrPair));
    RsPtrPair ra{reinterpret_cast<size_t>(a.fPtr), a.fIndex};
    RsPtrPair rb{reinterpret_cast<size_t>(b.fPtr), b.fIndex};
    return skia_rs_ptr_set_less(ra, rb);
}

uint32_t SkPtrSet::find(void* ptr) const {
    static_assert(sizeof(Pair) == sizeof(RsPtrPair));
    return skia_rs_ptr_set_find(reinterpret_cast<const RsPtrPair*>(fList.begin()),
                                SkToSizeT(fList.size()),
                                reinterpret_cast<size_t>(ptr));
}

uint32_t SkPtrSet::add(void* ptr) {
    static_assert(sizeof(Pair) == sizeof(RsPtrPair));
    if (nullptr == ptr) {
        return 0;
    }
    RsPtrInsertResult res = skia_rs_ptr_set_find_or_insert(
            reinterpret_cast<const RsPtrPair*>(fList.begin()),
            SkToSizeT(fList.size()),
            reinterpret_cast<size_t>(ptr));
    if (!res.found) {
        this->incPtr(ptr);
        Pair pair{ptr, res.index};
        *fList.insert(SkToInt(res.pos)) = pair;
    }
    return res.index;
}

void SkPtrSet::copyToArray(void* array[]) const {
    static_assert(sizeof(Pair) == sizeof(RsPtrPair));
    static_assert(sizeof(void*) == sizeof(size_t));
    int count = fList.size();
    if (count > 0) {
        SkASSERT(array);
        skia_rs_ptr_set_copy_to_array(reinterpret_cast<const RsPtrPair*>(fList.begin()),
                                      SkToSizeT(count),
                                      reinterpret_cast<size_t*>(array));
    }
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// SkPictureFlat (src/core/SkPictureFlat.cpp)
///////////////////////////////////////////////////////////////////////////////////////////////////

void SkTypefacePlayback::setCount(size_t count) {
    fCount = count;
    fArray = std::make_unique<sk_sp<SkTypeface>[]>(count);
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// SkPicture (src/core/SkPicture.cpp)
///////////////////////////////////////////////////////////////////////////////////////////////////

SkPicture::SkPicture(const SkRect& cull,
                     sk_sp<const SkRecord> record,
                     std::unique_ptr<const SkSnapshotArray> drawablePicts,
                     sk_sp<const SkBBoxHierarchy> bbh,
                     size_t approxBytesUsedBySubPictures)
        : fUniqueID(skia_rs_picture_next_id())
        , fCullRect(cull)
        , fApproxBytesUsedBySubPictures(approxBytesUsedBySubPictures)
        , fRecord(std::move(record))
        , fDrawablePicts(std::move(drawablePicts))
        , fBBH(std::move(bbh)) {}

SkPicture::~SkPicture() {
    if (fAddedToCache.load()) {
        SkResourceCache::PostPurgeSharedID(SkPicturePriv::MakeSharedID(fUniqueID));
    }
}

SkPictInfo SkPicture::createHeader() const {
    return skia_rs_picture_create_header(this->cullRect());
}

bool SkPicture::IsValidPictInfo(const SkPictInfo& info) {
    return skia_rs_picture_is_valid_pict_info(&info);
}

bool SkPicture::StreamIsSKP(SkStream* stream, SkPictInfo* pInfo) {
    if (!stream) {
        return false;
    }
    uint8_t raw[28];
    if (stream->read(raw, sizeof(raw)) != sizeof(raw)) {
        return false;
    }
    SkPictInfo info;
    bool valid = skia_rs_picture_decode_pict_info(raw, &info);
    if (pInfo) {
        *pInfo = info;
    }
    return valid;
}

bool SkPicture_StreamIsSKP(SkStream* stream, SkPictInfo* pInfo) {
    return SkPicture::StreamIsSKP(stream, pInfo);
}

bool SkPicture::BufferIsSKP(SkReadBuffer* buffer, SkPictInfo* pInfo) {
    SkPictInfo info;
    if (!buffer->readByteArray(&info.fMagic, sizeof(info.fMagic))) {
        return false;
    }
    uint32_t ver = buffer->readUInt();
    // Avoid SkASSERT(version != ~0U) on corrupt buffers by writing through raw bytes if needed.
    if (ver != ~0U) {
        info.setVersion(ver);
    }
    buffer->readRect(&info.fCullRect);

    if (IsValidPictInfo(info)) {
        if (pInfo) {
            *pInfo = info;
        }
        return true;
    }
    return false;
}

sk_sp<SkPicture> SkPicture::Forwardport(const SkPictInfo& info,
                                        const SkPictureData* data,
                                        SkReadBuffer* buffer) {
    if (!data || !data->opData()) {
        return nullptr;
    }
    SkPicturePlayback playback(data);
    SkPictureRecorder r;
    playback.draw(r.beginRecording(info.fCullRect), nullptr, buffer);
    return r.finishRecordingAsPicture();
}

sk_sp<SkPicture> SkPicture::MakeFromStream(SkStream* stream, const SkDeserialProcs* procs) {
    return MakeFromStreamPriv(stream, procs, nullptr, SkPicturePriv::kDefaultRecursionLimit);
}

sk_sp<SkPicture> SkPicture::MakeFromData(const void* data,
                                         size_t size,
                                         const SkDeserialProcs* procs) {
    if (!data) {
        return nullptr;
    }
    SkMemoryStream stream(data, size);
    return MakeFromStreamPriv(&stream, procs, nullptr, SkPicturePriv::kDefaultRecursionLimit);
}

sk_sp<SkPicture> SkPicture::MakeFromData(const SkData* data, const SkDeserialProcs* procs) {
    if (!data) {
        return nullptr;
    }
    SkMemoryStream stream(data->data(), data->size());
    return MakeFromStreamPriv(&stream, procs, nullptr, SkPicturePriv::kDefaultRecursionLimit);
}

sk_sp<SkPicture> SkPicture::MakeFromStreamPriv(SkStream* stream,
                                               const SkDeserialProcs* procsPtr,
                                               SkTypefacePlayback* typefaces,
                                               int recursionLimit) {
    if (recursionLimit <= 0) {
        return nullptr;
    }
    SkPictInfo info;
    if (!StreamIsSKP(stream, &info)) {
        return nullptr;
    }

    SkDeserialProcs procs;
    if (procsPtr) {
        procs = *procsPtr;
    }

    uint8_t trailingStreamByteAfterPictInfo;
    if (!stream->readU8(&trailingStreamByteAfterPictInfo)) {
        return nullptr;
    }
    switch (trailingStreamByteAfterPictInfo) {
        case kPictureData_TrailingStreamByteAfterPictInfo: {
            std::unique_ptr<SkPictureData> data(
                    SkPictureData::CreateFromStream(stream, info, procs, typefaces, recursionLimit));
            return Forwardport(info, data.get(), nullptr);
        }
        case kCustom_TrailingStreamByteAfterPictInfo: {
            int32_t ssize;
            if (!stream->readS32(&ssize) || ssize >= 0 || !procs.fPictureProc) {
                return nullptr;
            }
            size_t size = skia_rs_picture_negate_to_size_t(ssize);
            if (SkStreamPriv::RemainingLengthIsBelow(stream, size)) {
                return nullptr;
            }
            auto data = SkData::MakeUninitialized(size);
            if (stream->read(data->writable_data(), size) != size) {
                return nullptr;
            }
            return procs.fPictureProc(data->data(), size, procs.fPictureCtx);
        }
        default:
            break;
    }
    return nullptr;
}

sk_sp<SkPicture> SkPicturePriv::MakeFromBuffer(SkReadBuffer& buffer) {
    if (!buffer.isValid()) {
        return nullptr;
    }
    SkPictInfo info;
    if (!SkPicture::BufferIsSKP(&buffer, &info)) {
        return nullptr;
    }
    int32_t ssize = buffer.read32();
    if (ssize < 0) {
        const SkDeserialProcs& procs = buffer.getDeserialProcs();
        if (!procs.fPictureProc) {
            return nullptr;
        }
        size_t size = skia_rs_picture_negate_to_size_t(ssize);
        return procs.fPictureProc(buffer.skip(size), size, procs.fPictureCtx);
    }
    if (ssize != 1) {
        return nullptr;
    }
    std::unique_ptr<SkPictureData> data(SkPictureData::CreateFromBuffer(buffer, info));
    return SkPicture::Forwardport(info, data.get(), &buffer);
}

SkPictureData* SkPicture::backport() const {
    SkPictInfo info = this->createHeader();
    SkPictureRecord rec(info.fCullRect.roundOut(), 0);
    rec.beginRecording();
    this->playback(&rec);
    rec.endRecording();
    return new SkPictureData(rec, info);
}

void SkPicture::serialize(SkWStream* stream, const SkSerialProcs* procs) const {
    this->serialize(stream, procs, nullptr);
}

sk_sp<SkData> SkPicture::serialize(const SkSerialProcs* procs) const {
    SkDynamicMemoryWStream stream;
    this->serialize(&stream, procs, nullptr);
    return stream.detachAsData();
}

namespace {

sk_sp<const SkData> custom_serialize(const SkPicture* picture, const SkSerialProcs& procs) {
    if (procs.fPictureProc) {
        auto data = procs.fPictureProc(const_cast<SkPicture*>(picture), procs.fPictureCtx);
        if (data) {
            if (!skia_rs_picture_validate_custom_serialize_size(data->size())) {
                return SkData::MakeEmpty();
            }
            return data;
        }
    }
    return nullptr;
}

bool write_pad32(SkWStream* stream, const void* data, size_t size) {
    if (!stream->write(data, size)) {
        return false;
    }
    size_t pad = skia_rs_picture_pad32_bytes(size);
    if (pad != 0) {
        uint32_t zero = 0;
        return stream->write(&zero, pad);
    }
    return true;
}

struct NestedApproxOpCounter {
    SK_ONLY_STACK_ALLOCATABLE;

    int fCount = 0;
    int fDepth = 0;
    const SkSnapshotArray* fDrawablePicts;

    NestedApproxOpCounter(const SkSnapshotArray* drawablePicts, int depth)
            : fDepth(depth), fDrawablePicts(drawablePicts) {}

    template <typename T>
    void operator()(const T&) {
        fCount++;
    }

    void operator()(const SkRecords::DrawPicture& op) {
        fCount += 1 + SkPicturePriv::ApproximateOpCount(op.picture.get(), fDepth + 1, true);
    }

    void operator()(const SkRecords::DrawDrawable& op) {
        fCount++;
        if (!fDrawablePicts) {
            return;
        }
        if (0 <= op.index && op.index < fDrawablePicts->count()) {
            fCount += SkPicturePriv::ApproximateOpCount(
                    fDrawablePicts->at(op.index), fDepth + 1, true);
        }
    }
};

}  // namespace

void SkPicture::serialize(SkWStream* stream,
                          const SkSerialProcs* procsPtr,
                          SkRefCntSet* typefaceSet,
                          bool textBlobsOnly) const {
    SkSerialProcs procs;
    if (procsPtr) {
        procs = *procsPtr;
    }

    SkPictInfo info = this->createHeader();
    uint8_t headerBytes[28];
    skia_rs_picture_encode_pict_info(&info, headerBytes);
    stream->write(headerBytes, sizeof(headerBytes));

    if (auto custom = custom_serialize(this, procs)) {
        int32_t size = SkToS32(custom->size());
        if (size == 0) {
            stream->write8(kFailure_TrailingStreamByteAfterPictInfo);
            return;
        }
        stream->write8(kCustom_TrailingStreamByteAfterPictInfo);
        stream->write32(-size);
        write_pad32(stream, custom->data(), size);
        return;
    }

    std::unique_ptr<SkPictureData> data(this->backport());
    if (data) {
        stream->write8(kPictureData_TrailingStreamByteAfterPictInfo);
        data->serialize(stream, procs, typefaceSet, textBlobsOnly);
    } else {
        stream->write8(kFailure_TrailingStreamByteAfterPictInfo);
    }
}

void SkPicturePriv::Flatten(const sk_sp<const SkPicture> picture, SkWriteBuffer& buffer) {
    SkPictInfo info = picture->createHeader();
    std::unique_ptr<SkPictureData> data(picture->backport());

    buffer.writeByteArray(&info.fMagic, sizeof(info.fMagic));
    buffer.writeUInt(info.getVersion());
    buffer.writeRect(info.fCullRect);

    if (auto custom = custom_serialize(picture.get(), buffer.serialProcs())) {
        int32_t size = SkToS32(custom->size());
        buffer.write32(-size);
        buffer.writePad32(custom->data(), size);
        return;
    }

    if (data) {
        buffer.write32(1);
        data->flatten(buffer);
    } else {
        buffer.write32(0);
    }
}

sk_sp<SkPicture> SkPicturePriv::MakeEmptyPicture() {
    return SkPicturePriv::MakePicture(
            SkRect::MakeEmpty(), sk_make_sp<SkRecord>(), nullptr, nullptr, 0);
}

sk_sp<SkPicture> SkPicturePriv::MakePicture(const SkRect& cull,
                                            sk_sp<const SkRecord> record,
                                            std::unique_ptr<const SkSnapshotArray> drawablePicts,
                                            sk_sp<const SkBBoxHierarchy> bbh,
                                            size_t approxBytesUsedBySubPictures) {
    return sk_sp<SkPicture>(new SkPicture(cull,
                                          std::move(record),
                                          std::move(drawablePicts),
                                          std::move(bbh),
                                          approxBytesUsedBySubPictures));
}

void SkPicture::playback(SkCanvas* canvas, AbortCallback* callback) const {
    SkASSERT(canvas);
    if (this->isPlaceholder()) {
        return;
    }
    const bool useBBH = skia_rs_picture_should_use_bbh(canvas->getLocalClipBounds(), this->cullRect());
    SkRecordDraw(*fRecord,
                 canvas,
                 fDrawablePicts ? fDrawablePicts->begin() : nullptr,
                 nullptr,
                 fDrawablePicts ? fDrawablePicts->count() : 0,
                 useBBH ? fBBH.get() : nullptr,
                 callback);
}

int SkPicturePriv::ApproximateOpCount(const SkPicture* pic, int depth, bool nested) {
    if (!pic || depth > SkPicturePriv::kDefaultRecursionLimit) {
        return 0;
    }
    if (pic->isPlaceholder()) {
        return kMaxPictureOpsToUnrollInsteadOfRef + 1;
    }
    if (!nested) {
        return pic->fRecord->count();
    }
    NestedApproxOpCounter visitor(pic->fDrawablePicts.get(), depth);
    for (int i = 0; i < pic->fRecord->count(); i++) {
        pic->fRecord->visit(i, visitor);
    }
    return visitor.fCount;
}

int SkPicture::approximateOpCount(bool nested) const {
    return SkPicturePriv::ApproximateOpCount(this, 0, nested);
}

size_t SkPicture::approximateBytesUsed() const {
    return skia_rs_picture_approx_bytes_used(
            this->isPlaceholder(),
            sizeof(*this),
            fRecord ? fRecord->bytesUsed() : 0,
            fApproxBytesUsedBySubPictures,
            fBBH ? fBBH->bytesUsed() : 0);
}

sk_sp<SkPicture> SkPicture::MakePlaceholder(SkRect cull) {
    return SkPicturePriv::MakePicture(cull, nullptr, nullptr, nullptr, 0);
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// SkPictureData (src/core/SkPictureData.cpp)
///////////////////////////////////////////////////////////////////////////////////////////////////

SkPictureData::SkPictureData(const SkPictInfo& info) : fInfo(info) {}

SkPictureData::SkPictureData(const SkPictureRecord& record, const SkPictInfo& info)
        : fPictures(record.getPictures())
        , fDrawables(record.getDrawables())
        , fTextBlobs(record.getTextBlobs())
        , fVertices(record.getVertices())
        , fImages(record.getImages())
        , fSlugs(record.getSlugs())
        , fInfo(info) {
    fOpData = record.opData();
    fPaints = record.fPaints;

    fPaths.reset(record.fPaths.count());
    record.fPaths.foreach([this](const SkPath& path, int n) { fPaths[n - 1] = path; });
}

namespace {

void write_tag_size(SkWriteBuffer& buffer, uint32_t tag, size_t size) {
    buffer.writeUInt(tag);
    buffer.writeUInt(SkToU32(size));
}

void write_tag_size(SkWStream* stream, uint32_t tag, size_t size) {
    stream->write32(tag);
    stream->write32(SkToU32(size));
}

SkSerialProcs skip_typeface_proc(const SkSerialProcs& procs) {
    SkSerialProcs newProcs = procs;
    newProcs.fTypefaceProc = nullptr;
    newProcs.fTypefaceCtx = nullptr;
    return newProcs;
}

sk_sp<SkImage> create_image_from_buffer(SkReadBuffer& buffer) { return buffer.readImage(); }

sk_sp<SkDrawable> create_drawable_from_buffer(SkReadBuffer& buffer) {
    return sk_sp<SkDrawable>(
            (SkDrawable*)buffer.readFlattenable(SkFlattenable::kSkDrawable_Type));
}

template <typename U>
using FactoryFn = sk_sp<U> (*)(SkReadBuffer&);

template <typename T, typename U>
bool new_array_from_buffer(SkReadBuffer& buffer,
                           uint32_t inCount,
                           TArray<sk_sp<T>>& array,
                           FactoryFn<U> factory) {
    if (!buffer.validate(array.empty() && SkTFitsIn<int>(inCount))) {
        return false;
    }
    if (0 == inCount) {
        return true;
    }
    for (uint32_t i = 0; i < inCount; ++i) {
        auto obj = factory(buffer);
        if (!buffer.validate(obj != nullptr)) {
            array.clear();
            return false;
        }
        array.push_back(std::move(obj));
    }
    return true;
}

}  // namespace

void SkPictureData::WriteFactories(SkWStream* stream, const SkFactorySet& rec) {
    int count = rec.count();

    AutoSTMalloc<16, SkFlattenable::Factory> storage(count);
    SkFlattenable::Factory* array = storage.get();
    rec.copyToArray(array);

    AutoSTMalloc<16, size_t> nameLens(count);
    for (int i = 0; i < count; i++) {
        const char* name = SkFlattenable::FactoryToName(array[i]);
        nameLens[i] = (nullptr == name || 0 == *name) ? 0 : strlen(name);
    }

    size_t size = skia_rs_picture_compute_factory_chunk_size(nameLens.get(), SkToSizeT(count));
    write_tag_size(stream, SK_PICT_FACTORY_TAG, (uint32_t)size);
    SkDEBUGCODE(size_t start = stream->bytesWritten());
    stream->write32(count);

    for (int i = 0; i < count; i++) {
        const char* name = SkFlattenable::FactoryToName(array[i]);
        if (nullptr == name || 0 == *name) {
            stream->writePackedUInt(0);
        } else {
            size_t len = nameLens[i];
            stream->writePackedUInt(len);
            stream->write(name, len);
        }
    }

    SkASSERT(size == (stream->bytesWritten() - start));
}

void SkPictureData::WriteTypefaces(SkWStream* stream,
                                   const SkRefCntSet& rec,
                                   const SkSerialProcs& procs) {
    int count = rec.count();
    write_tag_size(stream, SK_PICT_TYPEFACE_TAG, count);

    AutoSTMalloc<16, SkTypeface*> storage(count);
    SkTypeface** array = storage.get();
    rec.copyToArray((SkRefCnt**)array);

    for (int i = 0; i < count; i++) {
        SkTypeface* tf = array[i];
        if (procs.fTypefaceProc) {
            auto data = procs.fTypefaceProc(tf, procs.fTypefaceCtx);
            if (data) {
                stream->write(data->data(), data->size());
                continue;
            }
        }
        tf->serialize(stream, SkTypeface::SerializeBehavior::kDoIncludeData);
    }
}

void SkPictureData::flattenToBuffer(SkWriteBuffer& buffer, bool textBlobsOnly) const {
    if (!textBlobsOnly) {
        int numPaints = fPaints.size();
        if (numPaints > 0) {
            write_tag_size(buffer, SK_PICT_PAINT_BUFFER_TAG, numPaints);
            for (const SkPaint& paint : fPaints) {
                buffer.writePaint(paint);
            }
        }

        int numPaths = fPaths.size();
        if (numPaths > 0) {
            write_tag_size(buffer, SK_PICT_PATH_BUFFER_TAG, numPaths);
            buffer.writeInt(numPaths);
            for (const SkPath& path : fPaths) {
                buffer.writePath(path);
            }
        }
    }

    if (!fTextBlobs.empty()) {
        write_tag_size(buffer, SK_PICT_TEXTBLOB_BUFFER_TAG, fTextBlobs.size());
        for (const auto& blob : fTextBlobs) {
            SkTextBlobPriv::Flatten(*blob, buffer);
        }
    }

    if (!textBlobsOnly) {
        write_tag_size(buffer, SK_PICT_SLUG_BUFFER_TAG, fSlugs.size());
        for (const auto& slug : fSlugs) {
            slug->doFlatten(buffer);
        }
    }

    if (!textBlobsOnly) {
        if (!fVertices.empty()) {
            write_tag_size(buffer, SK_PICT_VERTICES_BUFFER_TAG, fVertices.size());
            for (const auto& vert : fVertices) {
                vert->priv().encode(buffer);
            }
        }

        if (!fImages.empty()) {
            write_tag_size(buffer, SK_PICT_IMAGE_BUFFER_TAG, fImages.size());
            for (const auto& img : fImages) {
                buffer.writeImage(img.get());
            }
        }
    }
}

void SkPictureData::serialize(SkWStream* stream,
                              const SkSerialProcs& procs,
                              SkRefCntSet* topLevelTypeFaceSet,
                              bool textBlobsOnly) const {
    write_tag_size(stream, SK_PICT_READER_TAG, fOpData->size());
    stream->write(fOpData->bytes(), fOpData->size());

    SkRefCntSet localTypefaceSet;
    SkRefCntSet* typefaceSet = topLevelTypeFaceSet ? topLevelTypeFaceSet : &localTypefaceSet;

    SkFactorySet factSet;
    SkBinaryWriteBuffer buffer(skip_typeface_proc(procs));
    buffer.setFactoryRecorder(sk_ref_sp(&factSet));
    buffer.setTypefaceRecorder(sk_ref_sp(typefaceSet));
    this->flattenToBuffer(buffer, textBlobsOnly);

    struct DevNull : public SkWStream {
        DevNull() : fBytesWritten(0) {}
        size_t fBytesWritten;
        bool write(const void*, size_t size) override {
            fBytesWritten += size;
            return true;
        }
        size_t bytesWritten() const override { return fBytesWritten; }
    } devnull;
    for (const auto& pic : fPictures) {
        pic->serialize(&devnull, nullptr, typefaceSet, true);
    }
    if (textBlobsOnly) {
        return;
    }

    WriteFactories(stream, factSet);
    WriteTypefaces(stream, *typefaceSet, procs);

    write_tag_size(stream, SK_PICT_BUFFER_SIZE_TAG, buffer.bytesWritten());
    buffer.writeToStream(stream);

    if (!fPictures.empty()) {
        write_tag_size(stream, SK_PICT_PICTURE_TAG, fPictures.size());
        for (const auto& pic : fPictures) {
            pic->serialize(stream, &procs, typefaceSet, false);
        }
    }

    stream->write32(SK_PICT_EOF_TAG);
}

void SkPictureData::flatten(SkWriteBuffer& buffer) const {
    write_tag_size(buffer, SK_PICT_READER_TAG, fOpData->size());
    buffer.writeByteArray(fOpData->bytes(), fOpData->size());

    if (!fPictures.empty()) {
        write_tag_size(buffer, SK_PICT_PICTURE_TAG, fPictures.size());
        for (const auto& pic : fPictures) {
            SkPicturePriv::Flatten(pic, buffer);
        }
    }

    if (!fDrawables.empty()) {
        write_tag_size(buffer, SK_PICT_DRAWABLE_TAG, fDrawables.size());
        for (const auto& draw : fDrawables) {
            buffer.writeFlattenable(draw.get());
        }
    }

    this->flattenToBuffer(buffer, false);
    buffer.write32(SK_PICT_EOF_TAG);
}

bool SkPictureData::parseStreamTag(SkStream* stream,
                                   uint32_t tag,
                                   uint32_t size,
                                   const SkDeserialProcs& procs,
                                   SkTypefacePlayback* topLevelTFPlayback,
                                   int recursionLimit) {
    if (procs.fAllowTagsProc && !procs.fAllowTagsProc(tag, procs.fAllowTagsCtx)) {
        return size == 0;
    }
    switch (tag) {
        case SK_PICT_READER_TAG:
            SkASSERT(nullptr == fOpData);
            fOpData = SkData::MakeFromStream(stream, size);
            if (!fOpData) {
                return false;
            }
            break;
        case SK_PICT_FACTORY_TAG: {
            if (!stream->readU32(&size)) {
                return false;
            }
            if (SkStreamPriv::RemainingLengthIsBelow(stream, size)) {
                return false;
            }
            fFactoryPlayback = std::make_unique<SkFactoryPlayback>(size);
            for (size_t i = 0; i < size; i++) {
                SkString str;
                size_t len;
                if (!stream->readPackedUInt(&len)) {
                    return false;
                }
                if (SkStreamPriv::RemainingLengthIsBelow(stream, len)) {
                    return false;
                }
                str.resize(len);
                if (stream->read(str.data(), len) != len) {
                    return false;
                }
                fFactoryPlayback->base()[i] = SkFlattenable::NameToFactory(str.c_str());
            }
        } break;
        case SK_PICT_TYPEFACE_TAG: {
            if (SkStreamPriv::RemainingLengthIsBelow(stream, size)) {
                return false;
            }
            fTFPlayback.setCount(size);
            for (uint32_t i = 0; i < size; ++i) {
                if (stream->isAtEnd()) {
                    return false;
                }
                sk_sp<SkTypeface> tf;
                if (procs.fTypefaceStreamProc) {
                    tf = procs.fTypefaceStreamProc(*stream, procs.fTypefaceCtx);
                } else {
                    tf = SkTypeface::MakeDeserialize(stream, nullptr);
                }
                if (!tf) {
                    tf = SkTypeface::MakeEmpty();
                }
                fTFPlayback[i] = std::move(tf);
            }
        } break;
        case SK_PICT_PICTURE_TAG: {
            SkASSERT(fPictures.empty());
            if (SkStreamPriv::RemainingLengthIsBelow(stream, size)) {
                return false;
            }
            fPictures.reserve_exact(SkToInt(size));
            for (uint32_t i = 0; i < size; i++) {
                auto pic = SkPicture::MakeFromStreamPriv(
                        stream, &procs, topLevelTFPlayback, recursionLimit - 1);
                if (!pic) {
                    return false;
                }
                fPictures.push_back(std::move(pic));
            }
        } break;
        case SK_PICT_BUFFER_SIZE_TAG: {
            if (SkStreamPriv::RemainingLengthIsBelow(stream, size)) {
                return false;
            }
            SkAutoMalloc storage(size);
            if (stream->read(storage.get(), size) != size) {
                return false;
            }

            SkReadBuffer buffer(storage.get(), size);
            buffer.setVersion(fInfo.getVersion());
            buffer.setRecursionLimit(recursionLimit - 1);

            if (!fFactoryPlayback) {
                return false;
            }
            fFactoryPlayback->setupBuffer(buffer);
            buffer.setDeserialProcs(procs);

            if (fTFPlayback.count() > 0) {
                fTFPlayback.setupBuffer(buffer);
            } else {
                topLevelTFPlayback->setupBuffer(buffer);
            }

            while (!buffer.eof() && buffer.isValid()) {
                tag = buffer.readUInt();
                size = buffer.readUInt();
                this->parseBufferTag(buffer, tag, size);
            }
            if (!buffer.isValid()) {
                return false;
            }
        } break;
    }
    return true;
}

void SkPictureData::parseBufferTag(SkReadBuffer& buffer, uint32_t tag, uint32_t size) {
    if (!buffer.allowTags(tag)) {
        buffer.validate(size == 0);
        return;
    }
    switch (tag) {
        case SK_PICT_PAINT_BUFFER_TAG: {
            if (!buffer.validate(SkTFitsIn<int>(size))) {
                return;
            }
            const int count = SkToInt(size);
            for (int i = 0; i < count; ++i) {
                fPaints.push_back(buffer.readPaint());
                if (!buffer.isValid()) {
                    return;
                }
            }
        } break;
        case SK_PICT_PATH_BUFFER_TAG:
            if (size > 0) {
                const int count = buffer.readInt();
                if (!buffer.validate(count >= 0)) {
                    return;
                }
                for (int i = 0; i < count; i++) {
                    if (auto path = buffer.readPath()) {
                        fPaths.push_back(std::move(*path));
                    } else {
                        SkASSERT(!buffer.isValid());
                        return;
                    }
                }
            }
            break;
        case SK_PICT_TEXTBLOB_BUFFER_TAG:
            new_array_from_buffer(buffer, size, fTextBlobs, SkTextBlobPriv::MakeFromBuffer);
            break;
        case SK_PICT_SLUG_BUFFER_TAG:
            new_array_from_buffer(buffer, size, fSlugs, sktext::gpu::Slug::MakeFromBuffer);
            break;
        case SK_PICT_VERTICES_BUFFER_TAG:
            new_array_from_buffer(buffer, size, fVertices, SkVerticesPriv::Decode);
            break;
        case SK_PICT_IMAGE_BUFFER_TAG:
            new_array_from_buffer(buffer, size, fImages, create_image_from_buffer);
            break;
        case SK_PICT_READER_TAG: {
            if (!buffer.validateCanReadN<uint8_t>(size)) {
                return;
            }
            auto data(SkData::MakeUninitialized(size));
            if (!buffer.readByteArray(data->writable_data(), size) ||
                !buffer.validate(nullptr == fOpData)) {
                return;
            }
            SkASSERT(nullptr == fOpData);
            fOpData = std::move(data);
        } break;
        case SK_PICT_PICTURE_TAG:
            buffer.downLevel();
            new_array_from_buffer(buffer, size, fPictures, SkPicturePriv::MakeFromBuffer);
            buffer.upLevel();
            break;
        case SK_PICT_DRAWABLE_TAG:
            buffer.downLevel();
            new_array_from_buffer(buffer, size, fDrawables, create_drawable_from_buffer);
            buffer.upLevel();
            break;
        default:
            buffer.validate(false);
            break;
    }
}

SkPictureData* SkPictureData::CreateFromStream(SkStream* stream,
                                               const SkPictInfo& info,
                                               const SkDeserialProcs& procs,
                                               SkTypefacePlayback* topLevelTFPlayback,
                                               int recursionLimit) {
    if (recursionLimit <= 0) {
        return nullptr;
    }
    std::unique_ptr<SkPictureData> data(new SkPictureData(info));
    if (!topLevelTFPlayback) {
        topLevelTFPlayback = &data->fTFPlayback;
    }
    if (!data->parseStream(stream, procs, topLevelTFPlayback, recursionLimit)) {
        return nullptr;
    }
    return data.release();
}

SkPictureData* SkPictureData::CreateFromBuffer(SkReadBuffer& buffer, const SkPictInfo& info) {
    if (!buffer.isValid()) {
        return nullptr;
    }
    std::unique_ptr<SkPictureData> data(new SkPictureData(info));
    buffer.setVersion(info.getVersion());
    if (!data->parseBuffer(buffer)) {
        return nullptr;
    }
    return data.release();
}

bool SkPictureData::parseStream(SkStream* stream,
                                const SkDeserialProcs& procs,
                                SkTypefacePlayback* topLevelTFPlayback,
                                int recursionLimit) {
    for (;;) {
        uint32_t tag;
        if (!stream->readU32(&tag)) {
            return false;
        }
        if (SK_PICT_EOF_TAG == tag) {
            break;
        }
        uint32_t size;
        if (!stream->readU32(&size)) {
            return false;
        }
        if (!this->parseStreamTag(stream, tag, size, procs, topLevelTFPlayback, recursionLimit)) {
            return false;
        }
    }
    return true;
}

bool SkPictureData::parseBuffer(SkReadBuffer& buffer) {
    while (buffer.isValid()) {
        uint32_t tag = buffer.readUInt();
        if (SK_PICT_EOF_TAG == tag) {
            break;
        }
        this->parseBufferTag(buffer, tag, buffer.readUInt());
    }
    if (!buffer.validate(this->opData() != nullptr)) {
        return false;
    }
    return true;
}

const SkPaint* SkPictureData::optionalPaint(SkReadBuffer* reader) const {
    int index = reader->readInt();
    RsOptionalPaintResult res =
            skia_rs_picture_validate_optional_paint_index(index, fPaints.size());
    if (res == RsOptionalPaintResult::kNone) {
        return nullptr;
    }
    return reader->validate(res == RsOptionalPaintResult::kValid) ? &fPaints[index - 1] : nullptr;
}

const SkPaint& SkPictureData::requiredPaint(SkReadBuffer* reader) const {
    const SkPaint* paint = this->optionalPaint(reader);
    if (reader->validate(paint != nullptr)) {
        return *paint;
    }
    static const SkPaint& stub = *(new SkPaint);
    return stub;
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// SkPicturePlayback (src/core/SkPicturePlayback.cpp)
///////////////////////////////////////////////////////////////////////////////////////////////////

namespace {

const SkRect* get_rect_ptr(SkReadBuffer* reader, SkRect* storage) {
    if (reader->readBool()) {
        reader->readRect(storage);
        return storage;
    }
    return nullptr;
}

void validate_offsetToRestore(SkReadBuffer* reader, size_t offsetToRestore) {
    reader->validate(
            skia_rs_picture_validate_offset_to_restore(offsetToRestore, reader->offset()));
}

SkRegion::Op unpack_clip_region_op(SkReadBuffer* reader, uint32_t packed) {
    RsUnpackedClipRegionOp u =
            skia_rs_picture_clip_params_unpack_region_op(packed, reader->getVersion());
    reader->validate(u.valid);
    return static_cast<SkRegion::Op>(u.region_op);
}

bool do_clip_op(SkReadBuffer* reader, SkCanvas* canvas, SkRegion::Op op, SkClipOp* clipOpToUse) {
    switch (skia_rs_picture_map_region_op_to_clip_action(static_cast<uint32_t>(op))) {
        case RsClipOpAction::kDirectDifference:
            *clipOpToUse = SkClipOp::kDifference;
            return true;
        case RsClipOpAction::kDirectIntersect:
            *clipOpToUse = SkClipOp::kIntersect;
            return true;
        case RsClipOpAction::kResetThenIntersect:
            SkASSERT(reader->isVersionLT(SkPicturePriv::kNoExpandingClipOps));
            SkCanvasPriv::ResetClip(canvas);
            *clipOpToUse = SkClipOp::kIntersect;
            return true;
        case RsClipOpAction::kIgnore:
        default:
            SkASSERT(reader->isVersionLT(SkPicturePriv::kNoExpandingClipOps));
            return false;
    }
}

}  // namespace

void SkPicturePlayback::draw(SkCanvas* canvas,
                             SkPicture::AbortCallback* callback,
                             SkReadBuffer* buffer) {
    AutoResetOpID aroi(this);
    SkASSERT(0 == fCurOffset);

    SkReadBuffer reader(fPictureData->opData()->bytes(), fPictureData->opData()->size());
    reader.setVersion(fPictureData->info().getVersion());

    SkM44 initialMatrix = canvas->getLocalToDevice();
    SkAutoCanvasRestore acr(canvas, false);

    while (!reader.eof() && reader.isValid()) {
        if (callback && callback->abort()) {
            return;
        }

        fCurOffset = reader.offset();

        uint32_t bits = reader.readInt();
        RsUnpackedOpHeader hdr = skia_rs_picture_unpack_op_word(bits);
        uint32_t op = hdr.op;
        uint32_t size = hdr.size;
        bool valid = hdr.valid;
        if (hdr.needs_extra_size) {
            size = reader.readInt();
            valid = skia_rs_picture_validate_op_and_size(op, size);
        }

        if (!reader.validate(valid)) {
            return;
        }

        this->handleOp(&reader, (DrawType)op, size, canvas, initialMatrix);
    }

    if (buffer) {
        buffer->validate(reader.isValid());
    }
}

void SkPicturePlayback::handleOp(SkReadBuffer* reader,
                                 DrawType op,
                                 uint32_t size,
                                 SkCanvas* canvas,
                                 const SkM44& initialMatrix) {
#define BREAK_ON_READ_ERROR(r) \
    if (!r->isValid()) break

    switch (op) {
        case NOOP: {
            SkASSERT(size >= 4);
            reader->skip(size - 4);
        } break;
        case FLUSH:
            break;
        case CLIP_PATH: {
            const SkPath& path = fPictureData->getPath(reader);
            uint32_t packed = reader->readInt();
            SkRegion::Op rgnOp = unpack_clip_region_op(reader, packed);
            bool doAA = skia_rs_picture_clip_params_unpack_do_aa(packed);
            size_t offsetToRestore = reader->readInt();
            validate_offsetToRestore(reader, offsetToRestore);
            BREAK_ON_READ_ERROR(reader);

            SkClipOp clipOp;
            if (do_clip_op(reader, canvas, rgnOp, &clipOp)) {
                canvas->clipPath(path, clipOp, doAA);
            }
            if (canvas->isClipEmpty() && offsetToRestore) {
                reader->skip(offsetToRestore - reader->offset());
            }
        } break;
        case CLIP_REGION: {
            SkRegion region;
            reader->readRegion(&region);
            uint32_t packed = reader->readInt();
            SkRegion::Op rgnOp = unpack_clip_region_op(reader, packed);
            size_t offsetToRestore = reader->readInt();
            validate_offsetToRestore(reader, offsetToRestore);
            BREAK_ON_READ_ERROR(reader);

            SkClipOp clipOp;
            if (do_clip_op(reader, canvas, rgnOp, &clipOp)) {
                canvas->clipRegion(region, clipOp);
            }
            if (canvas->isClipEmpty() && offsetToRestore) {
                reader->skip(offsetToRestore - reader->offset());
            }
        } break;
        case CLIP_RECT: {
            SkRect rect;
            reader->readRect(&rect);
            uint32_t packed = reader->readInt();
            SkRegion::Op rgnOp = unpack_clip_region_op(reader, packed);
            bool doAA = skia_rs_picture_clip_params_unpack_do_aa(packed);
            size_t offsetToRestore = reader->readInt();
            validate_offsetToRestore(reader, offsetToRestore);
            BREAK_ON_READ_ERROR(reader);

            SkClipOp clipOp;
            if (do_clip_op(reader, canvas, rgnOp, &clipOp)) {
                canvas->clipRect(rect, clipOp, doAA);
            }
            if (canvas->isClipEmpty() && offsetToRestore) {
                reader->skip(offsetToRestore - reader->offset());
            }
        } break;
        case CLIP_RRECT: {
            SkRRect rrect;
            reader->readRRect(&rrect);
            uint32_t packed = reader->readInt();
            SkRegion::Op rgnOp = unpack_clip_region_op(reader, packed);
            bool doAA = skia_rs_picture_clip_params_unpack_do_aa(packed);
            size_t offsetToRestore = reader->readInt();
            validate_offsetToRestore(reader, offsetToRestore);
            BREAK_ON_READ_ERROR(reader);

            SkClipOp clipOp;
            if (do_clip_op(reader, canvas, rgnOp, &clipOp)) {
                canvas->clipRRect(rrect, clipOp, doAA);
            }
            if (canvas->isClipEmpty() && offsetToRestore) {
                reader->skip(offsetToRestore - reader->offset());
            }
        } break;
        case CLIP_SHADER_IN_PAINT: {
            const SkPaint& paint = fPictureData->requiredPaint(reader);
            SkClipOp clipOp = reader->checkRange(SkClipOp::kDifference, SkClipOp::kIntersect);
            BREAK_ON_READ_ERROR(reader);
            canvas->clipShader(paint.refShader(), clipOp);
        } break;
        case RESET_CLIP:
            SkCanvasPriv::ResetClip(canvas);
            break;
        case PUSH_CULL:
            break;
        case POP_CULL:
            break;
        case CONCAT: {
            SkMatrix matrix;
            reader->readMatrix(&matrix);
            BREAK_ON_READ_ERROR(reader);
            canvas->concat(matrix);
        } break;
        case CONCAT44: {
            const SkScalar* colMaj = reader->skipT<SkScalar>(16);
            BREAK_ON_READ_ERROR(reader);
            canvas->concat(SkM44::ColMajor(colMaj));
        } break;
        case DRAW_ANNOTATION: {
            SkRect rect;
            reader->readRect(&rect);
            SkString key;
            reader->readString(&key);
            sk_sp<SkData> data = reader->readByteArrayAsData();
            BREAK_ON_READ_ERROR(reader);
            SkASSERT(data);
            canvas->drawAnnotation(rect, key.c_str(), data.get());
        } break;
        case DRAW_ARC: {
            const SkPaint& paint = fPictureData->requiredPaint(reader);
            SkRect rect;
            reader->readRect(&rect);
            SkScalar startAngle = reader->readScalar();
            SkScalar sweepAngle = reader->readScalar();
            int useCenter = reader->readInt();
            BREAK_ON_READ_ERROR(reader);
            canvas->drawArc(rect, startAngle, sweepAngle, SkToBool(useCenter), paint);
        } break;
        case DRAW_ATLAS: {
            const SkPaint* paint = fPictureData->optionalPaint(reader);
            const SkImage* atlas = fPictureData->getImage(reader);
            const uint32_t flags = reader->readUInt();
            const size_t count = reader->readUInt();
            const SkRSXform* xform = (const SkRSXform*)reader->skip(count, sizeof(SkRSXform));
            const SkRect* tex = (const SkRect*)reader->skip(count, sizeof(SkRect));
            const SkColor* colors = nullptr;
            SkBlendMode mode = SkBlendMode::kDst;
            if (flags & DRAW_ATLAS_HAS_COLORS) {
                colors = (const SkColor*)reader->skip(count, sizeof(SkColor));
                mode = reader->read32LE(SkBlendMode::kLastMode);
                BREAK_ON_READ_ERROR(reader);
            }
            const SkRect* cull = nullptr;
            if (flags & DRAW_ATLAS_HAS_CULL) {
                cull = (const SkRect*)reader->skip(sizeof(SkRect));
            }
            BREAK_ON_READ_ERROR(reader);

            SkSamplingOptions sampling;
            if (flags & DRAW_ATLAS_HAS_SAMPLING) {
                sampling = reader->readSampling();
                BREAK_ON_READ_ERROR(reader);
            }
            canvas->drawAtlas(atlas,
                              {xform, count},
                              {tex, count},
                              {colors, colors ? count : 0},
                              mode,
                              sampling,
                              cull,
                              paint);
        } break;
        case DRAW_CLEAR: {
            auto c = reader->readInt();
            BREAK_ON_READ_ERROR(reader);
            canvas->clear(c);
        } break;
        case DRAW_DATA: {
            size_t length = reader->readInt();
            (void)reader->skip(length);
        } break;
        case DRAW_DRAWABLE: {
            auto* d = fPictureData->getDrawable(reader);
            BREAK_ON_READ_ERROR(reader);
            canvas->drawDrawable(d);
        } break;
        case DRAW_DRAWABLE_MATRIX: {
            SkMatrix matrix;
            reader->readMatrix(&matrix);
            SkDrawable* drawable = fPictureData->getDrawable(reader);
            BREAK_ON_READ_ERROR(reader);
            canvas->drawDrawable(drawable, &matrix);
        } break;
        case DRAW_DRRECT: {
            const SkPaint& paint = fPictureData->requiredPaint(reader);
            SkRRect outer, inner;
            reader->readRRect(&outer);
            reader->readRRect(&inner);
            BREAK_ON_READ_ERROR(reader);
            canvas->drawDRRect(outer, inner, paint);
        } break;
        case DRAW_EDGEAA_QUAD: {
            SkRect rect;
            reader->readRect(&rect);
            SkCanvas::QuadAAFlags aaFlags = static_cast<SkCanvas::QuadAAFlags>(reader->read32());
            SkColor4f color;
            reader->readColor4f(&color);
            SkBlendMode blend = reader->read32LE(SkBlendMode::kLastMode);
            BREAK_ON_READ_ERROR(reader);
            bool hasClip = reader->readInt();
            const SkPoint* clip = nullptr;
            if (hasClip) {
                clip = (const SkPoint*)reader->skip(4, sizeof(SkPoint));
            }
            BREAK_ON_READ_ERROR(reader);
            canvas->experimental_DrawEdgeAAQuad(rect, clip, aaFlags, color, blend);
        } break;
        case DRAW_EDGEAA_IMAGE_SET:
        case DRAW_EDGEAA_IMAGE_SET2: {
            int cnt = reader->readInt();
            if (!reader->validate(cnt >= 0)) {
                break;
            }
            const SkPaint* paint = fPictureData->optionalPaint(reader);

            SkSamplingOptions sampling;
            if (op == DRAW_EDGEAA_IMAGE_SET2) {
                sampling = reader->readSampling();
            } else {
                sampling = SkSamplingOptions(SkFilterMode::kNearest);
            }

            SkCanvas::SrcRectConstraint constraint = reader->checkRange(
                    SkCanvas::kStrict_SrcRectConstraint, SkCanvas::kFast_SrcRectConstraint);

            if (!reader->validate(
                        skia_rs_picture_validate_edge_aa_entries_avail(cnt, reader->available()))) {
                break;
            }

            int expectedClipPointCount = 0;
            int maxMatrixIndex = -1;
            AutoTArray<SkCanvas::ImageSetEntry> set(cnt);
            for (int i = 0; i < cnt && reader->isValid(); ++i) {
                set[i].fImage = sk_ref_sp(fPictureData->getImage(reader));
                reader->readRect(&set[i].fSrcRect);
                reader->readRect(&set[i].fDstRect);
                set[i].fMatrixIndex = reader->readInt();
                set[i].fAlpha = reader->readScalar();
                set[i].fAAFlags = reader->readUInt();
                set[i].fHasClip = reader->readInt();

                expectedClipPointCount += set[i].fHasClip ? 4 : 0;
                if (set[i].fMatrixIndex > maxMatrixIndex) {
                    maxMatrixIndex = set[i].fMatrixIndex;
                }
            }

            int dstClipPointCount = reader->readInt();
            const SkPoint* dstClips = nullptr;
            if (!reader->validate(dstClipPointCount >= 0) ||
                !reader->validate(expectedClipPointCount == dstClipPointCount)) {
                break;
            } else if (dstClipPointCount > 0) {
                dstClips = (const SkPoint*)reader->skip(dstClipPointCount, sizeof(SkPoint));
                if (dstClips == nullptr) {
                    break;
                }
            }
            int matrixCount = reader->readInt();
            if (!reader->validate(skia_rs_picture_validate_edge_aa_matrices_avail(
                        matrixCount, maxMatrixIndex, reader->available()))) {
                break;
            }
            TArray<SkMatrix> matrices(matrixCount);
            for (int i = 0; i < matrixCount && reader->isValid(); ++i) {
                reader->readMatrix(&matrices.push_back());
            }
            BREAK_ON_READ_ERROR(reader);

            canvas->experimental_DrawEdgeAAImageSet(
                    set.get(), cnt, dstClips, matrices.begin(), sampling, paint, constraint);
        } break;
        case DRAW_IMAGE: {
            const SkPaint* paint = fPictureData->optionalPaint(reader);
            const SkImage* image = fPictureData->getImage(reader);
            SkPoint loc;
            reader->readPoint(&loc);
            BREAK_ON_READ_ERROR(reader);
            canvas->drawImage(
                    image, loc.fX, loc.fY, SkSamplingOptions(SkFilterMode::kNearest), paint);
        } break;
        case DRAW_IMAGE2: {
            const SkPaint* paint = fPictureData->optionalPaint(reader);
            const SkImage* image = fPictureData->getImage(reader);
            SkPoint loc;
            reader->readPoint(&loc);
            SkSamplingOptions sampling = reader->readSampling();
            BREAK_ON_READ_ERROR(reader);
            canvas->drawImage(image, loc.fX, loc.fY, sampling, paint);
        } break;
        case DRAW_IMAGE_LATTICE: {
            const SkPaint* paint = fPictureData->optionalPaint(reader);
            const SkImage* image = fPictureData->getImage(reader);
            SkCanvas::Lattice lattice;
            reader->validate(SkCanvasPriv::ReadLattice(*reader, &lattice));
            const SkRect* dst = reader->skipT<SkRect>();
            BREAK_ON_READ_ERROR(reader);
            canvas->drawImageLattice(image, lattice, *dst, SkFilterMode::kNearest, paint);
        } break;
        case DRAW_IMAGE_LATTICE2: {
            const SkPaint* paint = fPictureData->optionalPaint(reader);
            const SkImage* image = fPictureData->getImage(reader);
            SkCanvas::Lattice lattice;
            reader->validate(SkCanvasPriv::ReadLattice(*reader, &lattice));
            const SkRect* dst = reader->skipT<SkRect>();
            SkFilterMode filter = reader->read32LE(SkFilterMode::kLinear);
            BREAK_ON_READ_ERROR(reader);
            canvas->drawImageLattice(image, lattice, *dst, filter, paint);
        } break;
        case DRAW_IMAGE_NINE: {
            const SkPaint* paint = fPictureData->optionalPaint(reader);
            const SkImage* image = fPictureData->getImage(reader);
            SkIRect center;
            reader->readIRect(&center);
            SkRect dst;
            reader->readRect(&dst);
            BREAK_ON_READ_ERROR(reader);
            canvas->drawImageNine(image, center, dst, SkFilterMode::kNearest, paint);
        } break;
        case DRAW_IMAGE_RECT: {
            const SkPaint* paint = fPictureData->optionalPaint(reader);
            const SkImage* image = fPictureData->getImage(reader);
            SkRect storage;
            const SkRect* src = get_rect_ptr(reader, &storage);
            SkRect dst;
            reader->readRect(&dst);
            SkCanvas::SrcRectConstraint constraint = SkCanvas::kStrict_SrcRectConstraint;
            if (DRAW_IMAGE_RECT == op) {
                constraint = reader->checkRange(
                        SkCanvas::kStrict_SrcRectConstraint, SkCanvas::kFast_SrcRectConstraint);
            }
            BREAK_ON_READ_ERROR(reader);

            auto sampling = SkSamplingOptions(SkFilterMode::kNearest);
            if (src) {
                canvas->drawImageRect(image, *src, dst, sampling, paint, constraint);
            } else {
                canvas->drawImageRect(image, dst, sampling, paint);
            }
        } break;
        case DRAW_IMAGE_RECT2: {
            const SkPaint* paint = fPictureData->optionalPaint(reader);
            const SkImage* image = fPictureData->getImage(reader);
            SkRect src = reader->readRect();
            SkRect dst = reader->readRect();
            SkSamplingOptions sampling = reader->readSampling();
            auto constraint = reader->read32LE(SkCanvas::kFast_SrcRectConstraint);
            BREAK_ON_READ_ERROR(reader);
            canvas->drawImageRect(image, src, dst, sampling, paint, constraint);
        } break;
        case DRAW_OVAL: {
            const SkPaint& paint = fPictureData->requiredPaint(reader);
            SkRect rect;
            reader->readRect(&rect);
            BREAK_ON_READ_ERROR(reader);
            canvas->drawOval(rect, paint);
        } break;
        case DRAW_PAINT: {
            const SkPaint& paint = fPictureData->requiredPaint(reader);
            BREAK_ON_READ_ERROR(reader);
            canvas->drawPaint(paint);
        } break;
        case DRAW_BEHIND_PAINT: {
            const SkPaint& paint = fPictureData->requiredPaint(reader);
            BREAK_ON_READ_ERROR(reader);
            SkCanvasPriv::DrawBehind(canvas, paint);
        } break;
        case DRAW_PATCH: {
            const SkPaint& paint = fPictureData->requiredPaint(reader);
            const SkPoint* cubics =
                    (const SkPoint*)reader->skip(SkPatchUtils::kNumCtrlPts, sizeof(SkPoint));
            uint32_t flag = reader->readInt();
            const SkColor* colors = nullptr;
            if (flag & DRAW_VERTICES_HAS_COLORS) {
                colors = (const SkColor*)reader->skip(SkPatchUtils::kNumCorners, sizeof(SkColor));
            }
            const SkPoint* texCoords = nullptr;
            if (flag & DRAW_VERTICES_HAS_TEXS) {
                texCoords =
                        (const SkPoint*)reader->skip(SkPatchUtils::kNumCorners, sizeof(SkPoint));
            }
            SkBlendMode bmode = SkBlendMode::kModulate;
            if (flag & DRAW_VERTICES_HAS_XFER) {
                unsigned mode = reader->readInt();
                if (mode <= (unsigned)SkBlendMode::kLastMode) {
                    bmode = (SkBlendMode)mode;
                }
            }
            BREAK_ON_READ_ERROR(reader);
            canvas->drawPatch(cubics, colors, texCoords, bmode, paint);
        } break;
        case DRAW_PATH: {
            const SkPaint& paint = fPictureData->requiredPaint(reader);
            const auto& path = fPictureData->getPath(reader);
            BREAK_ON_READ_ERROR(reader);
            canvas->drawPath(path, paint);
        } break;
        case DRAW_PICTURE: {
            const auto* pic = fPictureData->getPicture(reader);
            BREAK_ON_READ_ERROR(reader);
            canvas->drawPicture(pic);
        } break;
        case DRAW_PICTURE_MATRIX_PAINT: {
            const SkPaint* paint = fPictureData->optionalPaint(reader);
            SkMatrix matrix;
            reader->readMatrix(&matrix);
            const SkPicture* pic = fPictureData->getPicture(reader);
            BREAK_ON_READ_ERROR(reader);
            canvas->drawPicture(pic, &matrix, paint);
        } break;
        case DRAW_POINTS: {
            const SkPaint& paint = fPictureData->requiredPaint(reader);
            SkCanvas::PointMode mode = reader->checkRange(
                    SkCanvas::kPoints_PointMode, SkCanvas::kPolygon_PointMode);
            size_t count = reader->readInt();
            const SkPoint* pts = (const SkPoint*)reader->skip(count, sizeof(SkPoint));
            BREAK_ON_READ_ERROR(reader);
            canvas->drawPoints(mode, {pts, count}, paint);
        } break;
        case DRAW_RECT: {
            const SkPaint& paint = fPictureData->requiredPaint(reader);
            SkRect rect;
            reader->readRect(&rect);
            BREAK_ON_READ_ERROR(reader);
            canvas->drawRect(rect, paint);
        } break;
        case DRAW_REGION: {
            const SkPaint& paint = fPictureData->requiredPaint(reader);
            SkRegion region;
            reader->readRegion(&region);
            BREAK_ON_READ_ERROR(reader);
            canvas->drawRegion(region, paint);
        } break;
        case DRAW_RRECT: {
            const SkPaint& paint = fPictureData->requiredPaint(reader);
            SkRRect rrect;
            reader->readRRect(&rrect);
            BREAK_ON_READ_ERROR(reader);
            canvas->drawRRect(rrect, paint);
        } break;
        case DRAW_SHADOW_REC: {
            const auto& path = fPictureData->getPath(reader);
            SkDrawShadowRec rec;
            reader->readPoint3(&rec.fZPlaneParams);
            reader->readPoint3(&rec.fLightPos);
            rec.fLightRadius = reader->readScalar();
            rec.fAmbientColor = reader->read32();
            rec.fSpotColor = reader->read32();
            rec.fFlags = reader->read32();
            BREAK_ON_READ_ERROR(reader);
            canvas->private_draw_shadow_rec(path, rec);
        } break;
        case DRAW_TEXT_BLOB: {
            const SkPaint& paint = fPictureData->requiredPaint(reader);
            const SkTextBlob* blob = fPictureData->getTextBlob(reader);
            SkScalar x = reader->readScalar();
            SkScalar y = reader->readScalar();
            BREAK_ON_READ_ERROR(reader);
            canvas->drawTextBlob(blob, x, y, paint);
        } break;
        case DRAW_SLUG: {
            const SkPaint& paint = fPictureData->requiredPaint(reader);
            const sktext::gpu::Slug* slug = fPictureData->getSlug(reader);
            BREAK_ON_READ_ERROR(reader);
            canvas->drawSlug(slug, paint);
        } break;
        case DRAW_VERTICES_OBJECT: {
            const SkPaint& paint = fPictureData->requiredPaint(reader);
            const SkVertices* vertices = fPictureData->getVertices(reader);
            const int boneCount = reader->readInt();
            (void)reader->skip(boneCount, sizeof(SkVertices_DeprecatedBone));
            SkBlendMode bmode = reader->read32LE(SkBlendMode::kLastMode);
            BREAK_ON_READ_ERROR(reader);
            if (vertices) {
                canvas->drawVertices(vertices, bmode, paint);
            }
        } break;
        case RESTORE:
            canvas->restore();
            break;
        case ROTATE: {
            auto deg = reader->readScalar();
            canvas->rotate(deg);
        } break;
        case SAVE:
            canvas->save();
            break;
        case SAVE_BEHIND: {
            uint32_t flags = reader->readInt();
            const SkRect* subset = nullptr;
            SkRect storage;
            if (flags & SAVEBEHIND_HAS_SUBSET) {
                reader->readRect(&storage);
                subset = &storage;
            }
            SkCanvasPriv::SaveBehind(canvas, subset);
        } break;
        case SAVE_LAYER_SAVELAYERREC: {
            SkCanvas::SaveLayerRec rec(nullptr, nullptr, nullptr, 0);
            const uint32_t flatFlags = reader->readInt();
            SkRect bounds;
            skia_private::AutoSTArray<2, sk_sp<SkImageFilter>> filters;
            if (flatFlags & SAVELAYERREC_HAS_BOUNDS) {
                reader->readRect(&bounds);
                rec.fBounds = &bounds;
            }
            if (flatFlags & SAVELAYERREC_HAS_PAINT) {
                rec.fPaint = &fPictureData->requiredPaint(reader);
            }
            if (flatFlags & SAVELAYERREC_HAS_BACKDROP) {
                const SkPaint& paint = fPictureData->requiredPaint(reader);
                rec.fBackdrop = paint.getImageFilter();
            }
            if (flatFlags & SAVELAYERREC_HAS_FLAGS) {
                rec.fSaveLayerFlags = reader->readInt();
            }
            if (flatFlags & SAVELAYERREC_HAS_CLIPMASK_OBSOLETE) {
                (void)fPictureData->getImage(reader);
            }
            if (flatFlags & SAVELAYERREC_HAS_CLIPMATRIX_OBSOLETE) {
                SkMatrix clipMatrix_ignored;
                reader->readMatrix(&clipMatrix_ignored);
            }
            if (!reader->isVersionLT(SkPicturePriv::Version::kBackdropScaleFactor) &&
                (flatFlags & SAVELAYERREC_HAS_BACKDROP_SCALE)) {
                SkCanvasPriv::SetBackdropScaleFactor(&rec, reader->readScalar());
            }
            if (!reader->isVersionLT(SkPicturePriv::Version::kMultipleFiltersOnSaveLayer) &&
                (flatFlags & SAVELAYERREC_HAS_MULTIPLE_FILTERS)) {
                int filterCount = reader->readUInt();
                reader->validate(skia_rs_picture_validate_save_layer_filter_count(filterCount));
                BREAK_ON_READ_ERROR(reader);
                filters.reset(filterCount);
                for (int i = 0; i < filterCount; ++i) {
                    const SkPaint& paint = fPictureData->requiredPaint(reader);
                    filters[i] = paint.refImageFilter();
                }
                rec.fFilters = filters;
            }
            if (!reader->isVersionLT(SkPicturePriv::Version::kSaveLayerBackdropTileMode) &&
                (flatFlags & SAVELAYERREC_HAS_BACKDROP_TILEMODE)) {
                rec.fBackdropTileMode = reader->read32LE(SkTileMode::kLastTileMode);
            }
            BREAK_ON_READ_ERROR(reader);
            canvas->saveLayer(rec);
        } break;
        case SCALE: {
            SkScalar sx = reader->readScalar();
            SkScalar sy = reader->readScalar();
            canvas->scale(sx, sy);
        } break;
        case SET_M44: {
            SkM44 m;
            reader->read(&m);
            canvas->setMatrix(initialMatrix * m);
        } break;
        case SET_MATRIX: {
            SkMatrix matrix;
            reader->readMatrix(&matrix);
            canvas->setMatrix(initialMatrix * SkM44(matrix));
        } break;
        case SKEW: {
            SkScalar sx = reader->readScalar();
            SkScalar sy = reader->readScalar();
            canvas->skew(sx, sy);
        } break;
        case TRANSLATE: {
            SkScalar dx = reader->readScalar();
            SkScalar dy = reader->readScalar();
            canvas->translate(dx, dy);
        } break;
        default:
            reader->validate(false);
            break;
    }

#undef BREAK_ON_READ_ERROR
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// SkPictureRecord (src/core/SkPictureRecord.cpp)
///////////////////////////////////////////////////////////////////////////////////////////////////

SkPictureRecord::SkPictureRecord(const SkIRect& dimensions, uint32_t flags)
        : SkCanvasVirtualEnforcer<SkCanvas>(dimensions)
        , fRecordFlags(flags)
        , fInitialSaveCount(kNoInitialSave) {}

SkPictureRecord::SkPictureRecord(const SkISize& dimensions, uint32_t flags)
        : SkPictureRecord(SkIRect::MakeSize(dimensions), flags) {}

void SkPictureRecord::willSave() {
    fRestoreOffsetStack.push_back(-(int32_t)fWriter.bytesWritten());
    this->recordSave();
    this->SkCanvasVirtualEnforcer<SkCanvas>::willSave();
}

void SkPictureRecord::recordSave() {
    size_t size = sizeof(kUInt32Size);
    size_t initialOffset = this->addDraw(SAVE, &size);
    this->validate(initialOffset, size);
}

SkCanvas::SaveLayerStrategy SkPictureRecord::getSaveLayerStrategy(const SaveLayerRec& rec) {
    fRestoreOffsetStack.push_back(-(int32_t)fWriter.bytesWritten());
    this->recordSaveLayer(rec);
    (void)this->SkCanvasVirtualEnforcer<SkCanvas>::getSaveLayerStrategy(rec);
    return kNoLayer_SaveLayerStrategy;
}

bool SkPictureRecord::onDoSaveBehind(const SkRect* subset) {
    fRestoreOffsetStack.push_back(-(int32_t)fWriter.bytesWritten());

    size_t size = sizeof(kUInt32Size) + sizeof(uint32_t);
    uint32_t flags = 0;
    if (subset) {
        flags |= SAVEBEHIND_HAS_SUBSET;
        size += sizeof(*subset);
    }

    size_t initialOffset = this->addDraw(SAVE_BEHIND, &size);
    this->addInt(flags);
    if (subset) {
        this->addRect(*subset);
    }

    this->validate(initialOffset, size);
    return false;
}

void SkPictureRecord::recordSaveLayer(const SaveLayerRec& rec) {
    uint32_t filterCount = SkToU32(rec.fFilters.size());
    SkScalar backdropScale = SkCanvasPriv::GetBackdropScaleFactor(rec);
    RsSaveLayerFlatLayout layout = skia_rs_picture_compute_save_layer_flat_layout(
            rec.fBounds != nullptr,
            rec.fPaint != nullptr,
            rec.fBackdrop != nullptr,
            rec.fSaveLayerFlags,
            backdropScale,
            filterCount,
            rec.fBackdropTileMode == SkTileMode::kClamp);

    size_t size = layout.total_bytes;
    const uint32_t flatFlags = layout.flat_flags;
    const size_t initialOffset = this->addDraw(SAVE_LAYER_SAVELAYERREC, &size);
    this->addInt(flatFlags);
    if (flatFlags & SAVELAYERREC_HAS_BOUNDS) {
        this->addRect(*rec.fBounds);
    }
    if (flatFlags & SAVELAYERREC_HAS_PAINT) {
        this->addPaintPtr(rec.fPaint);
    }
    if (flatFlags & SAVELAYERREC_HAS_BACKDROP) {
        SkPaint paint;
        paint.setImageFilter(sk_ref_sp(const_cast<SkImageFilter*>(rec.fBackdrop)));
        this->addPaint(paint);
    }
    if (flatFlags & SAVELAYERREC_HAS_FLAGS) {
        this->addInt(rec.fSaveLayerFlags);
    }
    if (flatFlags & SAVELAYERREC_HAS_BACKDROP_SCALE) {
        this->addScalar(backdropScale);
    }
    if (flatFlags & SAVELAYERREC_HAS_MULTIPLE_FILTERS) {
        this->addInt(filterCount);
        for (uint32_t i = 0; i < filterCount; ++i) {
            SkPaint paint;
            paint.setImageFilter(rec.fFilters[i]);
            this->addPaint(paint);
        }
    }
    if (flatFlags & SAVELAYERREC_HAS_BACKDROP_TILEMODE) {
        this->addInt((int)rec.fBackdropTileMode);
    }
    this->validate(initialOffset, size);
}

void SkPictureRecord::willRestore() {
    if (fRestoreOffsetStack.empty()) {
        return;
    }
    this->recordRestore();
    fRestoreOffsetStack.pop_back();
    this->SkCanvasVirtualEnforcer<SkCanvas>::willRestore();
}

void SkPictureRecord::recordRestore(bool fillInSkips) {
    if (fillInSkips) {
        this->fillRestoreOffsetPlaceholdersForCurrentStackLevel((uint32_t)fWriter.bytesWritten());
    }
    size_t size = 1 * kUInt32Size;
    size_t initialOffset = this->addDraw(RESTORE, &size);
    this->validate(initialOffset, size);
}

void SkPictureRecord::recordTranslate(const SkMatrix& m) {
    SkASSERT(SkMatrix::kTranslate_Mask == m.getType());
    size_t size = 1 * kUInt32Size + 2 * sizeof(SkScalar);
    size_t initialOffset = this->addDraw(TRANSLATE, &size);
    this->addScalar(m.getTranslateX());
    this->addScalar(m.getTranslateY());
    this->validate(initialOffset, size);
}

void SkPictureRecord::recordScale(const SkMatrix& m) {
    SkASSERT(SkMatrix::kScale_Mask == m.getType());
    size_t size = 1 * kUInt32Size + 2 * sizeof(SkScalar);
    size_t initialOffset = this->addDraw(SCALE, &size);
    this->addScalar(m.getScaleX());
    this->addScalar(m.getScaleY());
    this->validate(initialOffset, size);
}

void SkPictureRecord::didConcat44(const SkM44& m) {
    this->validate(fWriter.bytesWritten(), 0);
    size_t size = kUInt32Size + 16 * sizeof(SkScalar);
    size_t initialOffset = this->addDraw(CONCAT44, &size);
    fWriter.write(SkMatrixPriv::M44ColMajor(m), 16 * sizeof(SkScalar));
    this->validate(initialOffset, size);
    this->SkCanvasVirtualEnforcer<SkCanvas>::didConcat44(m);
}

void SkPictureRecord::didSetM44(const SkM44& m) {
    this->validate(fWriter.bytesWritten(), 0);
    size_t size = kUInt32Size + 16 * sizeof(SkScalar);
    size_t initialOffset = this->addDraw(SET_M44, &size);
    fWriter.write(SkMatrixPriv::M44ColMajor(m), 16 * sizeof(SkScalar));
    this->validate(initialOffset, size);
    this->SkCanvasVirtualEnforcer<SkCanvas>::didSetM44(m);
}

void SkPictureRecord::didScale(SkScalar x, SkScalar y) {
    this->didConcat44(SkM44::Scale(x, y));
}

void SkPictureRecord::didTranslate(SkScalar x, SkScalar y) {
    this->didConcat44(SkM44::Translate(x, y));
}

void SkPictureRecord::recordConcat(const SkMatrix& matrix) {
    this->validate(fWriter.bytesWritten(), 0);
    size_t size = kUInt32Size + SkMatrixPriv::WriteToMemory(matrix, nullptr);
    size_t initialOffset = this->addDraw(CONCAT, &size);
    this->addMatrix(matrix);
    this->validate(initialOffset, size);
}

void SkPictureRecord::fillRestoreOffsetPlaceholdersForCurrentStackLevel(uint32_t restoreOffset) {
    int32_t offset = fRestoreOffsetStack.back();
    while (offset > 0) {
        uint32_t peek = fWriter.readTAt<uint32_t>(offset);
        fWriter.overwriteTAt(offset, restoreOffset);
        offset = peek;
    }
}

void SkPictureRecord::beginRecording() { fInitialSaveCount = this->save(); }

void SkPictureRecord::endRecording() {
    SkASSERT(kNoInitialSave != fInitialSaveCount);
    this->restoreToCount(fInitialSaveCount);
}

size_t SkPictureRecord::recordRestoreOffsetPlaceholder() {
    if (fRestoreOffsetStack.empty()) {
        return -1;
    }
    int32_t prevOffset = fRestoreOffsetStack.back();
    size_t offset = fWriter.bytesWritten();
    this->addInt(prevOffset);
    fRestoreOffsetStack.back() = SkToU32(offset);
    return offset;
}

void SkPictureRecord::onClipRect(const SkRect& rect, SkClipOp op, ClipEdgeStyle edgeStyle) {
    this->recordClipRect(rect, op, kSoft_ClipEdgeStyle == edgeStyle);
    this->SkCanvasVirtualEnforcer<SkCanvas>::onClipRect(rect, op, edgeStyle);
}

size_t SkPictureRecord::recordClipRect(const SkRect& rect, SkClipOp op, bool doAA) {
    size_t size = 1 * kUInt32Size + sizeof(rect) + 1 * kUInt32Size;
    if (!fRestoreOffsetStack.empty()) {
        size += kUInt32Size;
    }
    size_t initialOffset = this->addDraw(CLIP_RECT, &size);
    this->addRect(rect);
    this->addInt(skia_rs_picture_clip_params_pack(static_cast<uint32_t>(op), doAA));
    size_t offset = this->recordRestoreOffsetPlaceholder();
    this->validate(initialOffset, size);
    return offset;
}

void SkPictureRecord::onClipRRect(const SkRRect& rrect, SkClipOp op, ClipEdgeStyle edgeStyle) {
    this->recordClipRRect(rrect, op, kSoft_ClipEdgeStyle == edgeStyle);
    this->SkCanvasVirtualEnforcer<SkCanvas>::onClipRRect(rrect, op, edgeStyle);
}

size_t SkPictureRecord::recordClipRRect(const SkRRect& rrect, SkClipOp op, bool doAA) {
    size_t size = 1 * kUInt32Size + SkRRect::kSizeInMemory + 1 * kUInt32Size;
    if (!fRestoreOffsetStack.empty()) {
        size += kUInt32Size;
    }
    size_t initialOffset = this->addDraw(CLIP_RRECT, &size);
    this->addRRect(rrect);
    this->addInt(skia_rs_picture_clip_params_pack(static_cast<uint32_t>(op), doAA));
    size_t offset = recordRestoreOffsetPlaceholder();
    this->validate(initialOffset, size);
    return offset;
}

void SkPictureRecord::onClipPath(const SkPath& path, SkClipOp op, ClipEdgeStyle edgeStyle) {
    int pathID = this->addPathToHeap(path);
    this->recordClipPath(pathID, op, kSoft_ClipEdgeStyle == edgeStyle);
    this->SkCanvasVirtualEnforcer<SkCanvas>::onClipPath(path, op, edgeStyle);
}

size_t SkPictureRecord::recordClipPath(int pathID, SkClipOp op, bool doAA) {
    size_t size = 3 * kUInt32Size;
    if (!fRestoreOffsetStack.empty()) {
        size += kUInt32Size;
    }
    size_t initialOffset = this->addDraw(CLIP_PATH, &size);
    this->addInt(pathID);
    this->addInt(skia_rs_picture_clip_params_pack(static_cast<uint32_t>(op), doAA));
    size_t offset = recordRestoreOffsetPlaceholder();
    this->validate(initialOffset, size);
    return offset;
}

void SkPictureRecord::onClipShader(sk_sp<SkShader> cs, SkClipOp op) {
    SkPaint paint;
    paint.setShader(cs);
    size_t size = 3 * kUInt32Size;
    size_t initialOffset = this->addDraw(CLIP_SHADER_IN_PAINT, &size);
    this->addPaint(paint);
    this->addInt((int)op);
    this->validate(initialOffset, size);
    this->SkCanvasVirtualEnforcer<SkCanvas>::onClipShader(std::move(cs), op);
}

void SkPictureRecord::onClipRegion(const SkRegion& region, SkClipOp op) {
    this->recordClipRegion(region, op);
    this->SkCanvasVirtualEnforcer<SkCanvas>::onClipRegion(region, op);
}

size_t SkPictureRecord::recordClipRegion(const SkRegion& region, SkClipOp op) {
    size_t size = 2 * kUInt32Size + region.writeToMemory(nullptr);
    if (!fRestoreOffsetStack.empty()) {
        size += kUInt32Size;
    }
    size_t initialOffset = this->addDraw(CLIP_REGION, &size);
    this->addRegion(region);
    this->addInt(skia_rs_picture_clip_params_pack(static_cast<uint32_t>(op), false));
    size_t offset = this->recordRestoreOffsetPlaceholder();
    this->validate(initialOffset, size);
    return offset;
}

void SkPictureRecord::onResetClip() {
    if (!fRestoreOffsetStack.empty()) {
        this->fillRestoreOffsetPlaceholdersForCurrentStackLevel(0);
    }
    size_t size = sizeof(kUInt32Size);
    size_t initialOffset = this->addDraw(RESET_CLIP, &size);
    this->validate(initialOffset, size);
    this->SkCanvasVirtualEnforcer<SkCanvas>::onResetClip();
}

void SkPictureRecord::onDrawPaint(const SkPaint& paint) {
    size_t size = 2 * kUInt32Size;
    size_t initialOffset = this->addDraw(DRAW_PAINT, &size);
    this->addPaint(paint);
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawBehind(const SkPaint& paint) {
    size_t size = 2 * kUInt32Size;
    size_t initialOffset = this->addDraw(DRAW_BEHIND_PAINT, &size);
    this->addPaint(paint);
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawPoints(PointMode mode,
                                   size_t count,
                                   const SkPoint pts[],
                                   const SkPaint& paint) {
    size_t size = 4 * kUInt32Size + count * sizeof(SkPoint);
    size_t initialOffset = this->addDraw(DRAW_POINTS, &size);
    this->addPaint(paint);
    this->addInt(mode);
    this->addInt(SkToInt(count));
    fWriter.writeMul4(pts, count * sizeof(SkPoint));
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawOval(const SkRect& oval, const SkPaint& paint) {
    size_t size = 2 * kUInt32Size + sizeof(oval);
    size_t initialOffset = this->addDraw(DRAW_OVAL, &size);
    this->addPaint(paint);
    this->addRect(oval);
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawArc(const SkRect& oval,
                                SkScalar startAngle,
                                SkScalar sweepAngle,
                                bool useCenter,
                                const SkPaint& paint) {
    size_t size =
            2 * kUInt32Size + sizeof(oval) + sizeof(startAngle) + sizeof(sweepAngle) + sizeof(int);
    size_t initialOffset = this->addDraw(DRAW_ARC, &size);
    this->addPaint(paint);
    this->addRect(oval);
    this->addScalar(startAngle);
    this->addScalar(sweepAngle);
    this->addInt(useCenter);
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawRect(const SkRect& rect, const SkPaint& paint) {
    size_t size = 2 * kUInt32Size + sizeof(rect);
    size_t initialOffset = this->addDraw(DRAW_RECT, &size);
    this->addPaint(paint);
    this->addRect(rect);
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawRegion(const SkRegion& region, const SkPaint& paint) {
    size_t regionBytes = region.writeToMemory(nullptr);
    size_t size = 2 * kUInt32Size + regionBytes;
    size_t initialOffset = this->addDraw(DRAW_REGION, &size);
    this->addPaint(paint);
    fWriter.writeRegion(region);
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawRRect(const SkRRect& rrect, const SkPaint& paint) {
    size_t size = 2 * kUInt32Size + SkRRect::kSizeInMemory;
    size_t initialOffset = this->addDraw(DRAW_RRECT, &size);
    this->addPaint(paint);
    this->addRRect(rrect);
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawDRRect(const SkRRect& outer,
                                   const SkRRect& inner,
                                   const SkPaint& paint) {
    size_t size = 2 * kUInt32Size + SkRRect::kSizeInMemory * 2;
    size_t initialOffset = this->addDraw(DRAW_DRRECT, &size);
    this->addPaint(paint);
    this->addRRect(outer);
    this->addRRect(inner);
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawPath(const SkPath& path, const SkPaint& paint) {
    size_t size = 3 * kUInt32Size;
    size_t initialOffset = this->addDraw(DRAW_PATH, &size);
    this->addPaint(paint);
    this->addPath(path);
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawImage2(const SkImage* image,
                                   SkScalar x,
                                   SkScalar y,
                                   const SkSamplingOptions& sampling,
                                   const SkPaint* paint) {
    size_t size = 3 * kUInt32Size + 2 * sizeof(SkScalar) + SkSamplingPriv::FlatSize(sampling);
    size_t initialOffset = this->addDraw(DRAW_IMAGE2, &size);
    this->addPaintPtr(paint);
    this->addImage(image);
    this->addScalar(x);
    this->addScalar(y);
    this->addSampling(sampling);
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawImageRect2(const SkImage* image,
                                       const SkRect& src,
                                       const SkRect& dst,
                                       const SkSamplingOptions& sampling,
                                       const SkPaint* paint,
                                       SrcRectConstraint constraint) {
    size_t size = 3 * kUInt32Size + 2 * sizeof(dst) + SkSamplingPriv::FlatSize(sampling) +
                  kUInt32Size;
    size_t initialOffset = this->addDraw(DRAW_IMAGE_RECT2, &size);
    this->addPaintPtr(paint);
    this->addImage(image);
    this->addRect(src);
    this->addRect(dst);
    this->addSampling(sampling);
    this->addInt(constraint);
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawImageLattice2(const SkImage* image,
                                          const Lattice& lattice,
                                          const SkRect& dst,
                                          SkFilterMode filter,
                                          const SkPaint* paint) {
    size_t latticeSize = SkCanvasPriv::WriteLattice(nullptr, lattice);
    size_t size = 3 * kUInt32Size + latticeSize + sizeof(dst) + sizeof(uint32_t);
    size_t initialOffset = this->addDraw(DRAW_IMAGE_LATTICE2, &size);
    this->addPaintPtr(paint);
    this->addImage(image);
    (void)SkCanvasPriv::WriteLattice(fWriter.reservePad(latticeSize), lattice);
    this->addRect(dst);
    this->addInt(static_cast<uint32_t>(filter));
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawTextBlob(const SkTextBlob* blob,
                                     SkScalar x,
                                     SkScalar y,
                                     const SkPaint& paint) {
    size_t size = 3 * kUInt32Size + 2 * sizeof(SkScalar);
    size_t initialOffset = this->addDraw(DRAW_TEXT_BLOB, &size);
    this->addPaint(paint);
    this->addTextBlob(blob);
    this->addScalar(x);
    this->addScalar(y);
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawSlug(const sktext::gpu::Slug* slug, const SkPaint& paint) {
    size_t size = 3 * kUInt32Size;
    size_t initialOffset = this->addDraw(DRAW_SLUG, &size);
    this->addPaint(paint);
    this->addSlug(slug);
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawPicture(const SkPicture* picture,
                                    const SkMatrix* matrix,
                                    const SkPaint* paint) {
    size_t size = 2 * kUInt32Size;
    size_t initialOffset;
    if (nullptr == matrix && nullptr == paint) {
        initialOffset = this->addDraw(DRAW_PICTURE, &size);
        this->addPicture(picture);
    } else {
        const SkMatrix& m = matrix ? *matrix : SkMatrix::I();
        size += SkMatrixPriv::WriteToMemory(m, nullptr) + kUInt32Size;
        initialOffset = this->addDraw(DRAW_PICTURE_MATRIX_PAINT, &size);
        this->addPaintPtr(paint);
        this->addMatrix(m);
        this->addPicture(picture);
    }
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawDrawable(SkDrawable* drawable, const SkMatrix* matrix) {
    size_t size = 2 * kUInt32Size;
    size_t initialOffset;
    if (nullptr == matrix) {
        initialOffset = this->addDraw(DRAW_DRAWABLE, &size);
        this->addDrawable(drawable);
    } else {
        size += SkMatrixPriv::WriteToMemory(*matrix, nullptr);
        initialOffset = this->addDraw(DRAW_DRAWABLE_MATRIX, &size);
        this->addMatrix(*matrix);
        this->addDrawable(drawable);
    }
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawVerticesObject(const SkVertices* vertices,
                                           SkBlendMode mode,
                                           const SkPaint& paint) {
    size_t size = 5 * kUInt32Size;
    size_t initialOffset = this->addDraw(DRAW_VERTICES_OBJECT, &size);
    this->addPaint(paint);
    this->addVertices(vertices);
    this->addInt(0);
    this->addInt(static_cast<uint32_t>(mode));
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawPatch(const SkPoint cubics[12],
                                  const SkColor colors[4],
                                  const SkPoint texCoords[4],
                                  SkBlendMode bmode,
                                  const SkPaint& paint) {
    RsDrawPatchFlatLayout layout = skia_rs_picture_compute_draw_patch_flat_layout(
            colors != nullptr, texCoords != nullptr, SkBlendMode::kModulate == bmode);
    size_t size = layout.total_bytes;
    uint32_t flag = layout.flags;

    size_t initialOffset = this->addDraw(DRAW_PATCH, &size);
    this->addPaint(paint);
    this->addPatch(cubics);
    this->addInt(flag);

    if (colors) {
        fWriter.write(colors, SkPatchUtils::kNumCorners * sizeof(SkColor));
    }
    if (texCoords) {
        fWriter.write(texCoords, SkPatchUtils::kNumCorners * sizeof(SkPoint));
    }
    if (flag & DRAW_VERTICES_HAS_XFER) {
        this->addInt((int)bmode);
    }
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawAtlas2(const SkImage* atlas,
                                   const SkRSXform xform[],
                                   const SkRect tex[],
                                   const SkColor colors[],
                                   int count,
                                   SkBlendMode mode,
                                   const SkSamplingOptions& sampling,
                                   const SkRect* cull,
                                   const SkPaint* paint) {
    RsDrawAtlasFlatLayout layout = skia_rs_picture_compute_draw_atlas_flat_layout(
            SkToSizeT(count), colors != nullptr, cull != nullptr, SkSamplingPriv::FlatSize(sampling));
    size_t size = layout.total_bytes;
    uint32_t flags = layout.flags;

    size_t initialOffset = this->addDraw(DRAW_ATLAS, &size);
    this->addPaintPtr(paint);
    this->addImage(atlas);
    this->addInt(flags);
    this->addInt(count);
    fWriter.write(xform, count * sizeof(SkRSXform));
    fWriter.write(tex, count * sizeof(SkRect));

    if (colors) {
        fWriter.write(colors, count * sizeof(SkColor));
        this->addInt((int)mode);
    }
    if (cull) {
        fWriter.write(cull, sizeof(SkRect));
    }
    this->addSampling(sampling);
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawShadowRec(const SkPath& path, const SkDrawShadowRec& rec) {
    size_t size = 2 * kUInt32Size + 2 * sizeof(SkPoint3) + 1 * sizeof(SkScalar) + 3 * kUInt32Size;
    size_t initialOffset = this->addDraw(DRAW_SHADOW_REC, &size);
    this->addPath(path);
    fWriter.writePoint3(rec.fZPlaneParams);
    fWriter.writePoint3(rec.fLightPos);
    fWriter.writeScalar(rec.fLightRadius);
    fWriter.write32(rec.fAmbientColor);
    fWriter.write32(rec.fSpotColor);
    fWriter.write32(rec.fFlags);
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawAnnotation(const SkRect& rect, const char key[], SkData* value) {
    size_t keyLen = SkWriter32::WriteStringSize(key);
    size_t valueLen = SkWriter32::WriteDataSize(value);
    size_t size = 4 + sizeof(SkRect) + keyLen + valueLen;

    size_t initialOffset = this->addDraw(DRAW_ANNOTATION, &size);
    this->addRect(rect);
    fWriter.writeString(key);
    fWriter.writeData(value);
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawEdgeAAQuad(const SkRect& rect,
                                       const SkPoint clip[4],
                                       SkCanvas::QuadAAFlags aa,
                                       const SkColor4f& color,
                                       SkBlendMode mode) {
    size_t size = 4 * kUInt32Size + sizeof(SkColor4f) + sizeof(rect) +
                  (clip ? 4 : 0) * sizeof(SkPoint);
    size_t initialOffset = this->addDraw(DRAW_EDGEAA_QUAD, &size);
    this->addRect(rect);
    this->addInt((int)aa);
    fWriter.write(&color, sizeof(SkColor4f));
    this->addInt((int)mode);
    this->addInt(clip != nullptr);
    if (clip) {
        this->addPoints(clip, 4);
    }
    this->validate(initialOffset, size);
}

void SkPictureRecord::onDrawEdgeAAImageSet2(const SkCanvas::ImageSetEntry set[],
                                            int count,
                                            const SkPoint dstClips[],
                                            const SkMatrix preViewMatrices[],
                                            const SkSamplingOptions& sampling,
                                            const SkPaint* paint,
                                            SkCanvas::SrcRectConstraint constraint) {
    static constexpr size_t kMatrixSize = 9 * sizeof(SkScalar);
    int totalDstClipCount, totalMatrixCount;
    SkCanvasPriv::GetDstClipAndMatrixCounts(set, count, &totalDstClipCount, &totalMatrixCount);

    size_t size = 6 * kUInt32Size + sizeof(SkPoint) * totalDstClipCount +
                  kMatrixSize * totalMatrixCount +
                  (4 * kUInt32Size + 2 * sizeof(SkRect) + sizeof(SkScalar)) * count +
                  SkSamplingPriv::FlatSize(sampling);
    size_t initialOffset = this->addDraw(DRAW_EDGEAA_IMAGE_SET2, &size);
    this->addInt(count);
    this->addPaintPtr(paint);
    this->addSampling(sampling);
    this->addInt((int)constraint);
    for (int i = 0; i < count; ++i) {
        this->addImage(set[i].fImage.get());
        this->addRect(set[i].fSrcRect);
        this->addRect(set[i].fDstRect);
        this->addInt(set[i].fMatrixIndex);
        this->addScalar(set[i].fAlpha);
        this->addInt((int)set[i].fAAFlags);
        this->addInt(set[i].fHasClip);
    }
    this->addInt(totalDstClipCount);
    this->addPoints(dstClips, totalDstClipCount);
    this->addInt(totalMatrixCount);
    for (int i = 0; i < totalMatrixCount; ++i) {
        this->addMatrix(preViewMatrices[i]);
    }
    this->validate(initialOffset, size);
}

namespace {

template <typename T>
uint64_t object_dedup_key(const T* obj) {
    return static_cast<uint64_t>(obj->uniqueID());
}

template <>
uint64_t object_dedup_key(const SkDrawable* obj) {
    return reinterpret_cast<uintptr_t>(obj);
}

template <typename T>
int find_or_append(TArray<sk_sp<T>>& array, T* obj) {
    const int n = array.size();
    if (n > 0) {
        AutoSTMalloc<32, uint64_t> ids(n);
        for (int i = 0; i < n; ++i) {
            ids[i] = object_dedup_key<std::remove_const_t<T>>(array[i].get());
        }
        uint64_t key = object_dedup_key<std::remove_const_t<T>>(obj);
        int found = skia_rs_picture_find_id_linear(ids.get(), SkToSizeT(n), key);
        if (found >= 0) {
            return found;
        }
    }
    array.push_back(sk_ref_sp(obj));
    return array.size() - 1;
}

}  // namespace

sk_sp<SkSurface> SkPictureRecord::onNewSurface(const SkImageInfo&, const SkSurfaceProps&) {
    return nullptr;
}

void SkPictureRecord::addImage(const SkImage* image) {
    this->addInt(find_or_append(fImages, image));
}

void SkPictureRecord::addMatrix(const SkMatrix& matrix) { fWriter.writeMatrix(matrix); }

void SkPictureRecord::addPaintPtr(const SkPaint* paint) {
    if (paint) {
        fPaints.push_back(*paint);
        this->addInt(fPaints.size());
    } else {
        this->addInt(0);
    }
}

int SkPictureRecord::addPathToHeap(const SkPath& path) {
    if (int* n = fPaths.find(path)) {
        return *n;
    }
    int n = fPaths.count() + 1;
    fPaths.set(path, n);
    return n;
}

void SkPictureRecord::addPath(const SkPath& path) { this->addInt(this->addPathToHeap(path)); }

void SkPictureRecord::addPatch(const SkPoint cubics[12]) {
    fWriter.write(cubics, SkPatchUtils::kNumCtrlPts * sizeof(SkPoint));
}

void SkPictureRecord::addPicture(const SkPicture* picture) {
    this->addInt(find_or_append(fPictures, picture) + 1);
}

void SkPictureRecord::addDrawable(SkDrawable* drawable) {
    this->addInt(find_or_append(fDrawables, drawable) + 1);
}

void SkPictureRecord::addPoint(const SkPoint& point) { fWriter.writePoint(point); }

void SkPictureRecord::addPoints(const SkPoint pts[], int count) {
    fWriter.writeMul4(pts, count * sizeof(SkPoint));
}

void SkPictureRecord::addNoOp() {
    size_t size = kUInt32Size;
    this->addDraw(NOOP, &size);
}

void SkPictureRecord::addRect(const SkRect& rect) { fWriter.writeRect(rect); }

void SkPictureRecord::addRectPtr(const SkRect* rect) {
    if (fWriter.writeBool(rect != nullptr)) {
        fWriter.writeRect(*rect);
    }
}

void SkPictureRecord::addIRect(const SkIRect& rect) { fWriter.write(&rect, sizeof(rect)); }

void SkPictureRecord::addIRectPtr(const SkIRect* rect) {
    if (fWriter.writeBool(rect != nullptr)) {
        *(SkIRect*)fWriter.reserve(sizeof(SkIRect)) = *rect;
    }
}

void SkPictureRecord::addRRect(const SkRRect& rrect) { fWriter.writeRRect(rrect); }

void SkPictureRecord::addRegion(const SkRegion& region) { fWriter.writeRegion(region); }

void SkPictureRecord::addSampling(const SkSamplingOptions& sampling) {
    fWriter.writeSampling(sampling);
}

void SkPictureRecord::addText(const void* text, size_t byteLength) {
    addInt(SkToInt(byteLength));
    fWriter.writePad(text, byteLength);
}

void SkPictureRecord::addTextBlob(const SkTextBlob* blob) {
    this->addInt(find_or_append(fTextBlobs, blob) + 1);
}

void SkPictureRecord::addSlug(const sktext::gpu::Slug* slug) {
    this->addInt(find_or_append(fSlugs, slug) + 1);
}

void SkPictureRecord::addVertices(const SkVertices* vertices) {
    this->addInt(find_or_append(fVertices, vertices) + 1);
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// SkPictureRecorder (src/core/SkPictureRecorder.cpp)
///////////////////////////////////////////////////////////////////////////////////////////////////

SkPictureRecorder::SkPictureRecorder() {
    fActivelyRecording = false;
    fRecorder = std::make_unique<SkRecordCanvas>(nullptr, SkRect::MakeEmpty());
}

SkPictureRecorder::~SkPictureRecorder() {}

SkCanvas* SkPictureRecorder::beginRecording(const SkRect& userCullRect,
                                            sk_sp<SkBBoxHierarchy> bbh) {
    const SkRect cullRect = skia_rs_picture_normalize_recording_cull_rect(userCullRect);
    fCullRect = cullRect;
    fBBH = std::move(bbh);

    if (!fRecord) {
        fRecord.reset(new SkRecord);
    }
    fRecorder->reset(fRecord.get(), cullRect);
    fActivelyRecording = true;
    return this->getRecordingCanvas();
}

SkCanvas* SkPictureRecorder::beginRecording(const SkRect& bounds, SkBBHFactory* factory) {
    return this->beginRecording(bounds, factory ? (*factory)() : nullptr);
}

SkCanvas* SkPictureRecorder::getRecordingCanvas() {
    return fActivelyRecording ? fRecorder.get() : nullptr;
}

sk_sp<SkPicture> SkPictureRecorder::finishRecordingAsPicture() {
    fActivelyRecording = false;
    fRecorder->restoreToCount(1);

    if (fRecord->count() == 0) {
        return SkPicturePriv::MakeEmptyPicture();
    }

    SkRecordOptimize(fRecord.get());

    SkDrawableList* drawableList = fRecorder->getDrawableList();
    std::unique_ptr<SkSnapshotArray> pictList{
            drawableList ? drawableList->newDrawableSnapshot() : nullptr};

    if (fBBH) {
        AutoTArray<SkRect> bounds(fRecord->count());
        AutoTMalloc<SkBBoxHierarchy::Metadata> meta(fRecord->count());
        SkRecordFillBounds(fCullRect, *fRecord, bounds.data(), meta);

        fBBH->insert(bounds.data(), meta, fRecord->count());

        SkRect bbhBound = skia_rs_picture_compute_bbh_union(
                bounds.data(), SkToSizeT(fRecord->count()));
        SkASSERT((bbhBound.isEmpty() || fCullRect.contains(bbhBound)) ||
                 (bbhBound.isEmpty() && fCullRect.isEmpty()));
        fCullRect = bbhBound;
    }

    size_t subPictureBytes = fRecorder->approxBytesUsedBySubPictures();
    for (int i = 0; pictList && i < pictList->count(); i++) {
        subPictureBytes += pictList->begin()[i]->approximateBytesUsed();
    }

    return SkPicturePriv::MakePicture(
            fCullRect, std::move(fRecord), std::move(pictList), std::move(fBBH), subPictureBytes);
}

sk_sp<SkPicture> SkPictureRecorder::finishRecordingAsPictureWithCull(const SkRect& cullRect) {
    fCullRect = cullRect;
    return this->finishRecordingAsPicture();
}

void SkPictureRecorder::partialReplay(SkCanvas* canvas) const {
    if (nullptr == canvas) {
        return;
    }
    int drawableCount = 0;
    SkDrawable* const* drawables = nullptr;
    SkDrawableList* drawableList = fRecorder->getDrawableList();
    if (drawableList) {
        drawableCount = drawableList->count();
        drawables = drawableList->begin();
    }
    SkRecordDraw(*fRecord, canvas, nullptr, drawables, drawableCount, nullptr, nullptr);
}

sk_sp<SkDrawable> SkPictureRecorder::finishRecordingAsDrawable() {
    fActivelyRecording = false;
    fRecorder->restoreToCount(1);

    SkRecordOptimize(fRecord.get());

    if (fBBH) {
        AutoTArray<SkRect> bounds(fRecord->count());
        AutoTMalloc<SkBBoxHierarchy::Metadata> meta(fRecord->count());
        SkRecordFillBounds(fCullRect, *fRecord, bounds.data(), meta);
        fBBH->insert(bounds.data(), meta, fRecord->count());
    }

    return sk_make_sp<SkRecordedDrawable>(
            std::move(fRecord), std::move(fBBH), fRecorder->detachDrawableList(), fCullRect);
}
