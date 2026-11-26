/*
 * Safe Rust bridge for src/utils/SkClipStackUtils.cpp (Layer 2.3 skia-rs-region,
 * belongs to :skia / skia_utils_private).
 */

#include "src/utils/SkClipStackUtils.h"

#include "include/core/SkPath.h"
#include "include/core/SkPathTypes.h"
#include "include/pathops/SkPathOps.h"
#include "src/core/SkClipStack.h"

#include <cstdint>

extern "C" {
int32_t skia_rs_clip_stack_classify_as_path_step(int32_t dev_type, bool is_replace);
}  // extern "C"

SkPath SkClipStack_AsPath(const SkClipStack& cs) {
    SkPath path;
    path.setFillType(SkPathFillType::kInverseEvenOdd);

    SkClipStack::Iter iter(cs, SkClipStack::Iter::kBottom_IterStart);
    while (const SkClipStack::Element* element = iter.next()) {
        int32_t action = skia_rs_clip_stack_classify_as_path_step(
                static_cast<int32_t>(element->getDeviceSpaceType()),
                element->isReplaceOp());

        if (action == 0) {  // Skip (kShader)
            continue;
        }

        SkPath operand;
        if (action == 2 || action == 4) {  // ReplaceGeometry or OpGeometry
            operand = element->asDeviceSpacePath();
        }

        if (action == 1 || action == 2) {  // ReplaceEmpty or ReplaceGeometry
            path = operand;
        } else {
            SkClipOp elementOp = element->getOp();
            if (auto result = Op(path, operand, (SkPathOp)elementOp)) {
                path = *result;
            }
        }
    }
    return path;
}
