/*
 * Safe Rust bridge for src/core/SkRRect.cpp (Layer 1.2 skia-rs-geometry).
 */

#include "include/core/SkRRect.h"

#include "include/core/SkMatrix.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRect.h"
#include "include/core/SkString.h"
#include "include/private/SkDebug.h"
#include "src/core/SkBuffer.h"
#include "src/core/SkPathPriv.h"
#include "src/core/SkPathRawShapes.h"
#include "src/core/SkRRectPriv.h"
#include "src/core/SkStringUtils.h"

#include <cstddef>
#include <cstdint>

static_assert(sizeof(SkRRect) == 52);

extern "C" {
bool skia_rs_rrect_initialize_rect(SkRRect* rr, const SkRect* rect);
void skia_rs_rrect_set_oval(SkRRect* rr, const SkRect* oval);
void skia_rs_rrect_set_rect_xy(SkRRect* rr, const SkRect* rect, float x_rad, float y_rad);
void skia_rs_rrect_set_nine_patch(
        SkRRect* rr, const SkRect* rect, float left_rad, float top_rad,
        float right_rad, float bottom_rad);
void skia_rs_rrect_set_rect_radii(SkRRect* rr, const SkRect* rect, const SkVector radii[4]);
bool skia_rs_rrect_scale_radii(SkRRect* rr);
void skia_rs_rrect_compute_type(SkRRect* rr);
bool skia_rs_rrect_check_corner_containment(const SkRRect* rr, float x, float y);
bool skia_rs_rrect_contains_point(const SkRRect* rr, SkPoint pt);
bool skia_rs_rrect_contains_rect(const SkRRect* rr, const SkRect* rect);
void skia_rs_rrect_inset(const SkRRect* rr, float dx, float dy, SkRRect* dst);
void skia_rs_rrect_offset(SkRRect* rr, float dx, float dy);
bool skia_rs_rrect_is_valid(const SkRRect* rr);
bool skia_rs_rrect_are_rect_and_radii_valid(const SkRect* rect, const SkVector radii[4]);
bool skia_rs_rrect_is_nearly_simple_circular(const SkRRect* rr, float tolerance);
bool skia_rs_rrect_all_corners_circular(const SkRRect* rr, float tolerance);
bool skia_rs_rrect_is_relatively_circular(float rx, float ry, float tolerance);
bool skia_rs_rrect_all_corners_relatively_circular(const SkRRect* rr, float tolerance);
SkRect skia_rs_rrect_inner_bounds(const SkRRect* rr);
void skia_rs_rrect_conservative_intersect(const SkRRect* a, const SkRRect* b, SkRRect* out);
size_t skia_rs_rrect_write_to_memory(const SkRRect* rr, uint8_t* dst);
size_t skia_rs_rrect_read_from_memory(SkRRect* rr, const uint8_t* src, size_t len);
}  // extern "C"

void SkRRect::setOval(const SkRect& oval) {
    skia_rs_rrect_set_oval(this, &oval);
}

void SkRRect::setRectXY(const SkRect& rect, SkScalar xRad, SkScalar yRad) {
    skia_rs_rrect_set_rect_xy(this, &rect, xRad, yRad);
}

void SkRRect::setNinePatch(const SkRect& rect, SkScalar leftRad, SkScalar topRad,
                           SkScalar rightRad, SkScalar bottomRad) {
    skia_rs_rrect_set_nine_patch(this, &rect, leftRad, topRad, rightRad, bottomRad);
}

void SkRRect::setRectRadii(const SkRect& rect, const SkVector radii[4]) {
    skia_rs_rrect_set_rect_radii(this, &rect, radii);
}

bool SkRRect::initializeRect(const SkRect& rect) {
    return skia_rs_rrect_initialize_rect(this, &rect);
}

bool SkRRect::scaleRadii() {
    return skia_rs_rrect_scale_radii(this);
}

bool SkRRect::checkCornerContainment(SkScalar x, SkScalar y) const {
    return skia_rs_rrect_check_corner_containment(this, x, y);
}

bool SkRRectPriv::IsNearlySimpleCircular(const SkRRect& rr, float tolerance) {
    return skia_rs_rrect_is_nearly_simple_circular(&rr, tolerance);
}

bool SkRRectPriv::AllCornersCircular(const SkRRect& rr, float tolerance) {
    return skia_rs_rrect_all_corners_circular(&rr, tolerance);
}

bool SkRRectPriv::IsRelativelyCircular(float rx, float ry, float tolerance) {
    return skia_rs_rrect_is_relatively_circular(rx, ry, tolerance);
}

bool SkRRectPriv::AllCornersRelativelyCircular(const SkRRect& rr, float tolerance) {
    return skia_rs_rrect_all_corners_relatively_circular(&rr, tolerance);
}

bool SkRRect::contains(const SkPoint& point) const {
    return skia_rs_rrect_contains_point(this, point);
}

bool SkRRect::contains(const SkRect& rect) const {
    return skia_rs_rrect_contains_rect(this, &rect);
}

void SkRRect::computeType() {
    skia_rs_rrect_compute_type(this);
}

std::optional<SkRRect> SkRRect::transform(const SkMatrix& matrix) const {
    if (matrix.isIdentity()) {
        return *this;
    }
    if (!matrix.preservesAxisAlignment()) {
        return {};
    }
    if (this->isEmpty()) {
        return MakeEmpty();
    }
    const SkRect newRect = matrix.mapRect(fRect);
    if (!newRect.isFinite() || newRect.isEmpty()) {
        return {};
    }
    switch (this->getType()) {
        case kRect_Type: return MakeRect(newRect);
        case kOval_Type: return MakeOval(newRect);
        default: break;
    }
    SkPathRawShapes::RRect raw(*this);
    matrix.mapPoints(raw.fStorage);
    return SkPathPriv::DeduceRRectFromContour(newRect, raw.fPoints, raw.fVerbs);
}

bool SkRRect::transform(const SkMatrix& matrix, SkRRect* dst) const {
    if (auto rr = this->transform(matrix)) {
        if (dst) {
            *dst = *rr;
        }
        return true;
    }
    return false;
}

void SkRRect::inset(SkScalar dx, SkScalar dy, SkRRect* dst) const {
    skia_rs_rrect_inset(this, dx, dy, dst);
}

void SkRRect::offset(SkScalar dx, SkScalar dy) {
    skia_rs_rrect_offset(this, dx, dy);
}

size_t SkRRect::writeToMemory(void* buffer) const {
    return skia_rs_rrect_write_to_memory(this, static_cast<uint8_t*>(buffer));
}

void SkRRectPriv::WriteToBuffer(const SkRRect& rr, SkWBuffer* buffer) {
    uint8_t storage[SkRRect::kSizeInMemory];
    rr.writeToMemory(storage);
    buffer->write(storage, SkRRect::kSizeInMemory);
}

size_t SkRRect::readFromMemory(const void* buffer, size_t length) {
    return skia_rs_rrect_read_from_memory(this, static_cast<const uint8_t*>(buffer), length);
}

bool SkRRectPriv::ReadFromBuffer(SkRBuffer* buffer, SkRRect* rr) {
    if (buffer->available() < SkRRect::kSizeInMemory) {
        return false;
    }
    uint8_t storage[SkRRect::kSizeInMemory];
    return buffer->read(storage, SkRRect::kSizeInMemory) &&
           (rr->readFromMemory(storage, SkRRect::kSizeInMemory) == SkRRect::kSizeInMemory);
}

SkString SkRRect::dumpToString(bool asHex) const {
    SkScalarAsStringType asType = asHex ? kHex_SkScalarAsStringType : kDec_SkScalarAsStringType;
    SkString line = fRect.dumpToString(asHex);
    line.appendf("\nconst SkPoint corners[] = {\n");
    for (int i = 0; i < 4; ++i) {
        SkString strX, strY;
        SkAppendScalar(&strX, fRadii[i].x(), asType);
        SkAppendScalar(&strY, fRadii[i].y(), asType);
        line.appendf("    { %s, %s },", strX.c_str(), strY.c_str());
        if (asHex) {
            line.appendf(" /* %f %f */", fRadii[i].x(), fRadii[i].y());
        }
        line.append("\n");
    }
    line.append("};");
    return line;
}

void SkRRect::dump(bool asHex) const {
    SkDebugf("%s\n", this->dumpToString(asHex).c_str());
}

bool SkRRect::isValid() const {
    return skia_rs_rrect_is_valid(this);
}

bool SkRRectPriv::AreRectAndRadiiValid(const SkRect& rect, const SkVector radii[4]) {
    return skia_rs_rrect_are_rect_and_radii_valid(&rect, radii);
}

SkRect SkRRectPriv::InnerBounds(const SkRRect& rr) {
    return skia_rs_rrect_inner_bounds(&rr);
}

SkRRect SkRRectPriv::ConservativeIntersect(const SkRRect& a, const SkRRect& b) {
    SkRRect out;
    skia_rs_rrect_conservative_intersect(&a, &b, &out);
    return out;
}
