/*
 * Safe Rust bridge for src/core/SkVertices.cpp (Layer 1.2 skia-rs-geometry).
 */

#include "include/core/SkVertices.h"

#include "include/core/SkTypes.h"
#include "include/private/SkMalloc.h"
#include "include/private/SkTo.h"
#include "src/core/SkPicturePriv.h"
#include "src/core/SkReadBuffer.h"
#include "src/core/SkSafeRange.h"
#include "src/core/SkVerticesPriv.h"
#include "src/core/SkWriteBuffer.h"

#include <cstddef>
#include <cstdint>
#include <new>
#include <utility>

extern "C" {
struct SkiaRsVerticesSizes {
    size_t total;
    size_t arrays;
    size_t v_size;
    size_t t_size;
    size_t c_size;
    size_t i_size;
    size_t builder_tri_fan_i_size;
};

uint32_t skia_rs_vertices_next_id();
SkiaRsVerticesSizes skia_rs_vertices_compute_sizes(
        size_t obj_size, int32_t mode, int32_t vertex_count, int32_t index_count,
        bool has_texs, bool has_colors);
int32_t skia_rs_vertices_convert_tri_fan(
        const uint16_t* src_indices, int32_t src_index_count, int32_t vertex_count,
        uint16_t* dst_indices, size_t dst_cap);
void skia_rs_vertices_clamp_indices(
        const uint16_t* src, uint16_t* dst, size_t count, int32_t vertex_count);
bool skia_rs_vertices_validate_indices(
        const uint16_t* indices, size_t count, int32_t vertex_count);
}  // extern "C"

struct SkVertices::Desc {
    VertexMode fMode;
    int        fVertexCount,
               fIndexCount;
    bool       fHasTexs,
               fHasColors;
};

struct SkVertices::Sizes {
    Sizes(const Desc& desc) {
        SkiaRsVerticesSizes rs = skia_rs_vertices_compute_sizes(
                sizeof(SkVertices),
                static_cast<int32_t>(desc.fMode),
                desc.fVertexCount,
                desc.fIndexCount,
                desc.fHasTexs,
                desc.fHasColors);
        fTotal = rs.total;
        fArrays = rs.arrays;
        fVSize = rs.v_size;
        fTSize = rs.t_size;
        fCSize = rs.c_size;
        fISize = rs.i_size;
        fBuilderTriFanISize = rs.builder_tri_fan_i_size;
    }

    bool isValid() const {
        SkASSERT(fTotal >= fVSize);
        return fVSize > 0;
    }

    size_t fTotal = 0;
    size_t fArrays;
    size_t fVSize;
    size_t fTSize;
    size_t fCSize;
    size_t fISize;
    size_t fBuilderTriFanISize;
};

SkVertices::Builder::Builder(VertexMode mode, int vertexCount, int indexCount,
                             uint32_t builderFlags) {
    bool hasTexs = SkToBool(builderFlags & SkVertices::kHasTexCoords_BuilderFlag);
    bool hasColors = SkToBool(builderFlags & SkVertices::kHasColors_BuilderFlag);
    this->init({mode, vertexCount, indexCount, hasTexs, hasColors});
}

SkVertices::Builder::Builder(const Desc& desc) {
    this->init(desc);
}

void SkVertices::Builder::init(const Desc& desc) {
    Sizes sizes(desc);
    if (!sizes.isValid()) {
        SkASSERT(!this->isValid());
        return;
    }

    void* storage = ::operator new(sizes.fTotal);
    if (sizes.fBuilderTriFanISize) {
        fIntermediateFanIndices.reset(new uint8_t[sizes.fBuilderTriFanISize]);
    }

    fVertices.reset(new (storage) SkVertices);

    char* ptr = static_cast<char*>(storage) + sizeof(SkVertices);
    auto advance = [&ptr](size_t size) {
        char* new_ptr = size ? ptr : nullptr;
        ptr += size;
        return new_ptr;
    };

    fVertices->fPositions   = reinterpret_cast<SkPoint*>(advance(sizes.fVSize));
    fVertices->fTexs        = reinterpret_cast<SkPoint*>(advance(sizes.fTSize));
    fVertices->fColors      = reinterpret_cast<SkColor*>(advance(sizes.fCSize));
    fVertices->fIndices     = reinterpret_cast<uint16_t*>(advance(sizes.fISize));
    fVertices->fVertexCount = desc.fVertexCount;
    fVertices->fIndexCount  = desc.fIndexCount;
    fVertices->fMode        = desc.fMode;
}

sk_sp<SkVertices> SkVertices::Builder::detach() {
    if (fVertices) {
        fVertices->fBounds = SkRect::BoundsOrEmpty(
                {fVertices->fPositions, static_cast<size_t>(fVertices->fVertexCount)});
        if (fVertices->fMode == kTriangleFan_VertexMode) {
            const size_t dstCap = static_cast<size_t>(
                    3 * ((fIntermediateFanIndices ? fVertices->fIndexCount : fVertices->fVertexCount) - 2));
            fVertices->fIndexCount = skia_rs_vertices_convert_tri_fan(
                    fIntermediateFanIndices ? this->indices() : nullptr,
                    fVertices->fIndexCount,
                    fVertices->fVertexCount,
                    const_cast<uint16_t*>(fVertices->fIndices),
                    dstCap);
            fVertices->fMode = kTriangles_VertexMode;
        }
        fVertices->fUniqueID = skia_rs_vertices_next_id();
        return std::move(fVertices);
    }
    return nullptr;
}

SkPoint* SkVertices::Builder::positions() {
    return fVertices ? const_cast<SkPoint*>(fVertices->fPositions) : nullptr;
}

SkPoint* SkVertices::Builder::texCoords() {
    return fVertices ? const_cast<SkPoint*>(fVertices->fTexs) : nullptr;
}

SkColor* SkVertices::Builder::colors() {
    return fVertices ? const_cast<SkColor*>(fVertices->fColors) : nullptr;
}

uint16_t* SkVertices::Builder::indices() {
    if (!fVertices) {
        return nullptr;
    }
    if (fIntermediateFanIndices) {
        return reinterpret_cast<uint16_t*>(fIntermediateFanIndices.get());
    }
    return const_cast<uint16_t*>(fVertices->fIndices);
}

sk_sp<SkVertices> SkVertices::MakeCopy(VertexMode mode, int vertexCount,
                                       const SkPoint pos[], const SkPoint texs[],
                                       const SkColor colors[],
                                       int indexCount, const uint16_t indices[]) {
    auto desc = Desc{mode, vertexCount, indexCount, !!texs, !!colors};
    Builder builder(desc);
    if (!builder.isValid()) {
        return nullptr;
    }

    Sizes sizes(desc);
    sk_careful_memcpy(builder.positions(), pos, sizes.fVSize);
    sk_careful_memcpy(builder.texCoords(), texs, sizes.fTSize);
    sk_careful_memcpy(builder.colors(), colors, sizes.fCSize);

    const size_t isize = (mode == kTriangleFan_VertexMode) ? sizes.fBuilderTriFanISize
                                                           : sizes.fISize;
    const size_t icount = isize / sizeof(uint16_t);
    skia_rs_vertices_clamp_indices(indices, builder.indices(), icount, vertexCount);

    return builder.detach();
}

size_t SkVertices::approximateSize() const {
    return this->getSizes().fTotal;
}

SkVertices::Sizes SkVertices::getSizes() const {
    Sizes sizes({fMode, fVertexCount, fIndexCount, !!fTexs, !!fColors});
    SkASSERT(sizes.isValid());
    return sizes;
}

#define kMode_Mask          0x0FF
#define kHasTexs_Mask       0x100
#define kHasColors_Mask     0x200

void SkVerticesPriv::encode(SkWriteBuffer& buffer) const {
    uint32_t packed = static_cast<uint32_t>(fVertices->fMode);
    if (fVertices->fTexs) {
        packed |= kHasTexs_Mask;
    }
    if (fVertices->fColors) {
        packed |= kHasColors_Mask;
    }

    SkVertices::Sizes sizes = fVertices->getSizes();
    buffer.writeUInt(packed);
    buffer.writeInt(fVertices->fVertexCount);
    buffer.writeInt(fVertices->fIndexCount);

    buffer.writeByteArray(fVertices->fPositions, sizes.fVSize);
    buffer.writeByteArray(fVertices->fTexs, sizes.fTSize);
    buffer.writeByteArray(fVertices->fColors, sizes.fCSize);
    buffer.writeByteArray(fVertices->fIndices, sizes.fISize);
}

sk_sp<SkVertices> SkVerticesPriv::Decode(SkReadBuffer& buffer) {
    auto decode = [](SkReadBuffer& buffer) -> sk_sp<SkVertices> {
        SkSafeRange safe;
        bool hasCustomData = buffer.isVersionLT(SkPicturePriv::kVerticesRemoveCustomData_Version);

        const uint32_t packed = buffer.readUInt();
        const int vertexCount = safe.checkGE(buffer.readInt(), 0);
        const int indexCount = safe.checkGE(buffer.readInt(), 0);
        const int attrCount = hasCustomData ? safe.checkGE(buffer.readInt(), 0) : 0;
        const SkVertices::VertexMode mode = safe.checkLE<SkVertices::VertexMode>(
                packed & kMode_Mask, SkVertices::kLast_VertexMode);
        const bool hasTexs = SkToBool(packed & kHasTexs_Mask);
        const bool hasColors = SkToBool(packed & kHasColors_Mask);

        if (!safe || !buffer.isValid() || attrCount ||
            mode == SkVertices::kTriangleFan_VertexMode) {
            return nullptr;
        }

        const SkVertices::Desc desc{mode, vertexCount, indexCount, hasTexs, hasColors};
        SkVertices::Sizes sizes(desc);
        if (!sizes.isValid() || sizes.fArrays > buffer.available()) {
            return nullptr;
        }

        SkVertices::Builder builder(desc);
        if (!builder.isValid()) {
            return nullptr;
        }

        buffer.readByteArray(builder.positions(), sizes.fVSize);
        if (hasCustomData) {
            size_t customDataSize = 0;
            buffer.skipByteArray(&customDataSize);
            if (customDataSize != 0) {
                return nullptr;
            }
        }
        buffer.readByteArray(builder.texCoords(), sizes.fTSize);
        buffer.readByteArray(builder.colors(), sizes.fCSize);
        buffer.readByteArray(builder.indices(), sizes.fISize);

        if (!buffer.isValid()) {
            return nullptr;
        }

        if (indexCount > 0 &&
            !skia_rs_vertices_validate_indices(
                    builder.indices(), static_cast<size_t>(indexCount), vertexCount)) {
            return nullptr;
        }

        return builder.detach();
    };

    if (auto verts = decode(buffer)) {
        return verts;
    }
    buffer.validate(false);
    return nullptr;
}

void SkVertices::operator delete(void* p) {
    ::operator delete(p);
}
