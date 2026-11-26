/*
 * Safe Rust bridge for:
 *   src/utils/SkTextUtils.cpp
 * (Phase 1 Layer 5.1 skia-rs-text, GN target :skia).
 */

#include "include/utils/SkTextUtils.h"

#include "include/core/SkCanvas.h"
#include "include/core/SkFont.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPath.h"
#include "include/core/SkPathBuilder.h"
#include "include/core/SkPoint.h"
#include "include/core/SkScalar.h"
#include "include/core/SkTextBlob.h"
#include "include/private/SkOnlyStackAllocatable.h"
#include "include/private/SkTemplates.h"
#include "src/core/SkFontPriv.h"

#include <cstdint>

using namespace skia_private;

namespace {
extern "C" {
float skia_rs_text_utils_align_x(float x, float width, int32_t align);
}
}  // namespace

void SkTextUtils::Draw(SkCanvas* canvas, const void* text, size_t size, SkTextEncoding encoding,
                       SkScalar x, SkScalar y, const SkFont& font, const SkPaint& paint,
                       Align align) {
    if (align != kLeft_Align) {
        SkScalar width = font.measureText(text, size, encoding);
        x = skia_rs_text_utils_align_x(x, width, static_cast<int32_t>(align));
    }

    canvas->drawTextBlob(SkTextBlob::MakeFromText(text, size, font, encoding), x, y, paint);
}

void SkTextUtils::GetPath(const void* text, size_t length, SkTextEncoding encoding,
                          SkScalar x, SkScalar y, const SkFont& font, SkPath* path) {
    SkAutoToGlyphs ag(font, text, length, encoding);
    AutoTArray<SkPoint> pos(ag.count());
    font.getPos(ag.glyphs(), pos, {x, y});

    struct Rec {
        SK_ONLY_STACK_ALLOCATABLE;
        SkPathBuilder fDst;
        const SkPoint* fPos;
    } rec = {{}, pos.get()};

    font.getPaths(ag.glyphs(), [](const SkPath* src, const SkMatrix& mx, void* ctx) {
        Rec* rec = (Rec*)ctx;
        if (src) {
            SkMatrix m(mx);
            m.postTranslate(rec->fPos->fX, rec->fPos->fY);
            rec->fDst.addPath(*src, m);
        }
        rec->fPos += 1;
    }, &rec);
    *path = rec.fDst.detach();
}
