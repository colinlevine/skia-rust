/*
 * Safe Rust bridge for src/core/SkClipStack.cpp (Layer 2.3 skia-rs-region).
 */

#include "src/core/SkClipStack.h"

#include "include/core/SkBlendMode.h"
#include "include/core/SkPath.h"
#include "include/core/SkPathBuilder.h"
#include "include/core/SkScalar.h"
#include "include/private/SkDebug.h"
#include "src/core/SkRectPriv.h"

#include <array>
#include <cstddef>
#include <cstdint>
#include <new>
#include <utility>

static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkIRect) == 16);

extern "C" {
struct SkiaRsPriorBoundInfo {
    SkRect finite_bound;
    int32_t finite_bound_type;
    bool is_intersection_of_rects;
    SkRect device_space_rect;
    bool do_aa;
};
static_assert(sizeof(SkiaRsPriorBoundInfo) == 44);

struct SkiaRsBoundUpdateResult {
    SkRect finite_bound;
    int32_t finite_bound_type;
    bool is_intersection_of_rects;
    uint32_t gen_id;
    bool became_empty;
};
static_assert(sizeof(SkiaRsBoundUpdateResult) == 32);

struct SkiaRsPushDecision {
    int32_t action;
    SkRect merged_rect;
};
static_assert(sizeof(SkiaRsPushDecision) == 20);

uint32_t skia_rs_clip_stack_next_gen_id();
bool skia_rs_clip_stack_can_be_intersected_in_place(
        int32_t prior_type, int32_t prior_save_count, int32_t prior_op,
        bool prior_is_replace, int32_t save_count, int32_t op);
bool skia_rs_clip_stack_rect_rect_intersect_allowed(
        const SkRect* existing, bool existing_aa, const SkRect* new_rect, bool new_aa);
void skia_rs_clip_stack_combine_bounds_diff(
        int32_t combination, const SkRect* prev_finite, SkRect* finite_bound,
        int32_t* finite_bound_type, uint32_t* gen_id);
bool skia_rs_clip_stack_combine_bounds_intersection(
        int32_t combination, const SkRect* prev_finite, SkRect* finite_bound,
        int32_t* finite_bound_type);
void skia_rs_clip_stack_compute_bound_and_gen_id(
        int32_t cur_type, const SkRect* cur_bounds, int32_t cur_op, bool cur_do_aa,
        bool cur_is_replace, bool cur_path_is_inverse,
        const SkiaRsPriorBoundInfo* prior, SkiaRsBoundUpdateResult* out);
SkiaRsPushDecision skia_rs_clip_stack_decide_push(
        bool has_prior, int32_t prior_type, int32_t prior_save_count, int32_t prior_op,
        bool prior_is_replace, bool prior_do_aa, const SkRect* prior_bounds,
        int32_t save_count, int32_t new_type, int32_t new_op, bool new_is_replace,
        bool new_do_aa, const SkRect* new_bounds);
SkRect skia_rs_clip_stack_compute_clipped_bounds(
        const SkRect* finite_bound, int32_t bound_type, const SkIRect* device_bounds);
SkRect skia_rs_clip_stack_compute_conservative_bounds(
        const SkRect* finite_bound, int32_t bound_type,
        int32_t offset_x, int32_t offset_y, int32_t max_width, int32_t max_height);
uint32_t skia_rs_clip_stack_compute_topmost_gen_id(
        bool is_empty_deque, int32_t back_type, int32_t back_bound_type,
        const SkRect* back_finite_bound, uint32_t back_gen_id);
}  // extern "C"

// ===========================================================================
// SkClipStack::Element
// ===========================================================================

SkClipStack::Element::Element(const Element& that) {
    switch (that.getDeviceSpaceType()) {
        case DeviceSpaceType::kEmpty:
            fDeviceSpaceRRect.setEmpty();
            fDeviceSpacePath.reset();
            fShader.reset();
            break;
        case DeviceSpaceType::kRect:
        case DeviceSpaceType::kRRect:
            fDeviceSpacePath.reset();
            fShader.reset();
            fDeviceSpaceRRect = that.fDeviceSpaceRRect;
            break;
        case DeviceSpaceType::kPath:
            fShader.reset();
            fDeviceSpacePath = that.getDeviceSpacePath();
            break;
        case DeviceSpaceType::kShader:
            fDeviceSpacePath.reset();
            fShader = that.fShader;
            break;
    }

    fSaveCount = that.fSaveCount;
    fOp = that.fOp;
    fDeviceSpaceType = that.fDeviceSpaceType;
    fDoAA = that.fDoAA;
    fIsReplace = that.fIsReplace;
    fFiniteBoundType = that.fFiniteBoundType;
    fFiniteBound = that.fFiniteBound;
    fIsIntersectionOfRects = that.fIsIntersectionOfRects;
    fGenID = that.fGenID;
}

SkClipStack::Element::~Element() = default;

bool SkClipStack::Element::operator==(const Element& element) const {
    if (this == &element) {
        return true;
    }
    if (fOp != element.fOp || fDeviceSpaceType != element.fDeviceSpaceType ||
        fDoAA != element.fDoAA || fIsReplace != element.fIsReplace ||
        fSaveCount != element.fSaveCount) {
        return false;
    }
    switch (fDeviceSpaceType) {
        case DeviceSpaceType::kShader:
            return this->getShader() == element.getShader();
        case DeviceSpaceType::kPath:
            return this->getDeviceSpacePath() == element.getDeviceSpacePath();
        case DeviceSpaceType::kRRect:
            return fDeviceSpaceRRect == element.fDeviceSpaceRRect;
        case DeviceSpaceType::kRect:
            return this->getDeviceSpaceRect() == element.getDeviceSpaceRect();
        case DeviceSpaceType::kEmpty:
            return true;
        default:
            SkDEBUGFAIL("Unexpected type.");
            return false;
    }
}

const SkRect& SkClipStack::Element::getBounds() const {
    static const SkRect kEmpty = {0, 0, 0, 0};
    static const SkRect kInfinite = SkRectPriv::MakeLargeS32();
    switch (fDeviceSpaceType) {
        case DeviceSpaceType::kRect:
        case DeviceSpaceType::kRRect:
            return fDeviceSpaceRRect.getBounds();
        case DeviceSpaceType::kPath:
            return fDeviceSpacePath->getBounds();
        case DeviceSpaceType::kShader:
            return kInfinite;
        case DeviceSpaceType::kEmpty:
            return kEmpty;
        default:
            SkDEBUGFAIL("Unexpected type.");
            return kEmpty;
    }
}

bool SkClipStack::Element::contains(const SkRect& rect) const {
    switch (fDeviceSpaceType) {
        case DeviceSpaceType::kRect:
            return this->getDeviceSpaceRect().contains(rect);
        case DeviceSpaceType::kRRect:
            return fDeviceSpaceRRect.contains(rect);
        case DeviceSpaceType::kPath:
            return fDeviceSpacePath->conservativelyContainsRect(rect);
        case DeviceSpaceType::kEmpty:
        case DeviceSpaceType::kShader:
            return false;
        default:
            SkDEBUGFAIL("Unexpected type.");
            return false;
    }
}

bool SkClipStack::Element::contains(const SkRRect& rrect) const {
    switch (fDeviceSpaceType) {
        case DeviceSpaceType::kRect:
            return this->getDeviceSpaceRect().contains(rrect.getBounds());
        case DeviceSpaceType::kRRect:
            return fDeviceSpaceRRect.contains(rrect.getBounds()) || rrect == fDeviceSpaceRRect;
        case DeviceSpaceType::kPath:
            return fDeviceSpacePath->conservativelyContainsRect(rrect.getBounds());
        case DeviceSpaceType::kEmpty:
        case DeviceSpaceType::kShader:
            return false;
        default:
            SkDEBUGFAIL("Unexpected type.");
            return false;
    }
}

void SkClipStack::Element::initCommon(int saveCount, SkClipOp op, bool doAA) {
    fSaveCount = saveCount;
    fOp = op;
    fDoAA = doAA;
    fIsReplace = false;
    fFiniteBoundType = kInsideOut_BoundsType;
    fFiniteBound.setEmpty();
    fIsIntersectionOfRects = false;
    fGenID = kInvalidGenID;
}

void SkClipStack::Element::initRect(int saveCount, const SkRect& rect, const SkMatrix& m,
                                    SkClipOp op, bool doAA) {
    if (m.rectStaysRect()) {
        SkRect devRect;
        m.mapRect(&devRect, rect);
        fDeviceSpaceRRect.setRect(devRect);
        fDeviceSpaceType = DeviceSpaceType::kRect;
        this->initCommon(saveCount, op, doAA);
        return;
    }
    this->initAsPath(saveCount, SkPath::Rect(rect), m, op, doAA);
}

void SkClipStack::Element::initRRect(int saveCount, const SkRRect& rrect, const SkMatrix& m,
                                     SkClipOp op, bool doAA) {
    if (auto rr = rrect.transform(m)) {
        fDeviceSpaceRRect = *rr;
        SkRRect::Type type = fDeviceSpaceRRect.getType();
        if (SkRRect::kRect_Type == type || SkRRect::kEmpty_Type == type) {
            fDeviceSpaceType = DeviceSpaceType::kRect;
        } else {
            fDeviceSpaceType = DeviceSpaceType::kRRect;
        }
        this->initCommon(saveCount, op, doAA);
        return;
    }
    this->initAsPath(saveCount, SkPath::RRect(rrect), m, op, doAA);
}

void SkClipStack::Element::initPath(int saveCount, const SkPath& path, const SkMatrix& m,
                                    SkClipOp op, bool doAA) {
    if (!path.isInverseFillType()) {
        SkRect r;
        if (path.isRect(&r)) {
            this->initRect(saveCount, r, m, op, doAA);
            return;
        }
        SkRect ovalRect;
        if (path.isOval(&ovalRect)) {
            SkRRect rrect;
            rrect.setOval(ovalRect);
            this->initRRect(saveCount, rrect, m, op, doAA);
            return;
        }
    }
    this->initAsPath(saveCount, path, m, op, doAA);
}

void SkClipStack::Element::initAsPath(int saveCount, const SkPath& path, const SkMatrix& m,
                                      SkClipOp op, bool doAA) {
    SkPathBuilder builder(path);
    builder.transform(m);
    builder.setIsVolatile(true);
    fDeviceSpacePath = builder.detach();

    fDeviceSpaceType = DeviceSpaceType::kPath;
    this->initCommon(saveCount, op, doAA);
}

void SkClipStack::Element::initShader(int saveCount, sk_sp<SkShader> shader) {
    SkASSERT(shader);
    fDeviceSpaceType = DeviceSpaceType::kShader;
    fShader = std::move(shader);
    this->initCommon(saveCount, SkClipOp::kIntersect, false);
}

void SkClipStack::Element::initReplaceRect(int saveCount, const SkRect& rect, bool doAA) {
    fDeviceSpaceRRect.setRect(rect);
    fDeviceSpaceType = DeviceSpaceType::kRect;
    this->initCommon(saveCount, SkClipOp::kIntersect, doAA);
    fIsReplace = true;
}

SkPath SkClipStack::Element::asDeviceSpacePath() const {
    SkPathBuilder builder;
    switch (fDeviceSpaceType) {
        case DeviceSpaceType::kEmpty:
            break;
        case DeviceSpaceType::kRect:
            builder.addRect(this->getDeviceSpaceRect());
            break;
        case DeviceSpaceType::kRRect:
            builder.addRRect(fDeviceSpaceRRect);
            break;
        case DeviceSpaceType::kPath:
            return *fDeviceSpacePath;
        case DeviceSpaceType::kShader:
            builder.addRect(SkRectPriv::MakeLargeS32());
            break;
    }
    return builder.detach();
}

void SkClipStack::Element::setEmpty() {
    fDeviceSpaceType = DeviceSpaceType::kEmpty;
    fFiniteBound.setEmpty();
    fFiniteBoundType = kNormal_BoundsType;
    fIsIntersectionOfRects = false;
    fDeviceSpaceRRect.setEmpty();
    fDeviceSpacePath.reset();
    fShader.reset();
    fGenID = kEmptyGenID;
    SkDEBUGCODE(this->checkEmpty();)
}

void SkClipStack::Element::checkEmpty() const {
    SkASSERT(fFiniteBound.isEmpty());
    SkASSERT(kNormal_BoundsType == fFiniteBoundType);
    SkASSERT(!fIsIntersectionOfRects);
    SkASSERT(kEmptyGenID == fGenID);
    SkASSERT(fDeviceSpaceRRect.isEmpty());
    SkASSERT(!fDeviceSpacePath.has_value());
    SkASSERT(!fShader);
}

bool SkClipStack::Element::canBeIntersectedInPlace(int saveCount, SkClipOp op) const {
    return skia_rs_clip_stack_can_be_intersected_in_place(
            static_cast<int32_t>(fDeviceSpaceType), fSaveCount,
            static_cast<int32_t>(fOp), fIsReplace, saveCount, static_cast<int32_t>(op));
}

bool SkClipStack::Element::rectRectIntersectAllowed(const SkRect& newR, bool newAA) const {
    SkASSERT(DeviceSpaceType::kRect == fDeviceSpaceType);
    const SkRect& r = this->getDeviceSpaceRect();
    return skia_rs_clip_stack_rect_rect_intersect_allowed(&r, fDoAA, &newR, newAA);
}

void SkClipStack::Element::combineBoundsDiff(FillCombo combination, const SkRect& prevFinite) {
    int32_t bt = static_cast<int32_t>(fFiniteBoundType);
    skia_rs_clip_stack_combine_bounds_diff(
            static_cast<int32_t>(combination), &prevFinite, &fFiniteBound, &bt, &fGenID);
    fFiniteBoundType = static_cast<BoundsType>(bt);
}

void SkClipStack::Element::combineBoundsIntersection(int combination, const SkRect& prevFinite) {
    int32_t bt = static_cast<int32_t>(fFiniteBoundType);
    bool becameEmpty = skia_rs_clip_stack_combine_bounds_intersection(
            combination, &prevFinite, &fFiniteBound, &bt);
    fFiniteBoundType = static_cast<BoundsType>(bt);
    if (becameEmpty) {
        this->setEmpty();
    }
}

void SkClipStack::Element::updateBoundAndGenID(const Element* prior) {
    SkiaRsPriorBoundInfo priorInfo;
    const SkiaRsPriorBoundInfo* priorPtr = nullptr;
    if (prior) {
        priorInfo.finite_bound = prior->fFiniteBound;
        priorInfo.finite_bound_type = static_cast<int32_t>(prior->fFiniteBoundType);
        priorInfo.is_intersection_of_rects = prior->fIsIntersectionOfRects;
        priorInfo.device_space_rect = (prior->fDeviceSpaceType == DeviceSpaceType::kRect)
                ? prior->getDeviceSpaceRect()
                : SkRect::MakeEmpty();
        priorInfo.do_aa = prior->fDoAA;
        priorPtr = &priorInfo;
    }

    const SkRect& bounds = this->getBounds();
    const bool pathIsInverse = this->isInverseFilled();
    SkiaRsBoundUpdateResult res;
    skia_rs_clip_stack_compute_bound_and_gen_id(
            static_cast<int32_t>(fDeviceSpaceType),
            &bounds,
            static_cast<int32_t>(fOp),
            fDoAA,
            fIsReplace,
            pathIsInverse,
            priorPtr,
            &res);

    if (res.became_empty) {
        this->setEmpty();
    } else {
        fFiniteBound = res.finite_bound;
        fFiniteBoundType = static_cast<BoundsType>(res.finite_bound_type);
        fIsIntersectionOfRects = res.is_intersection_of_rects;
        fGenID = res.gen_id;
    }
}

// ===========================================================================
// SkClipStack
// ===========================================================================

static const int kDefaultElementAllocCnt = 8;

SkClipStack::SkClipStack()
    : fDeque(sizeof(Element), kDefaultElementAllocCnt)
    , fSaveCount(0) {
}

SkClipStack::SkClipStack(void* storage, size_t size)
    : fDeque(sizeof(Element), storage, size, kDefaultElementAllocCnt)
    , fSaveCount(0) {
}

SkClipStack::SkClipStack(const SkClipStack& b)
    : fDeque(sizeof(Element), kDefaultElementAllocCnt) {
    *this = b;
}

SkClipStack::~SkClipStack() {
    reset();
}

SkClipStack& SkClipStack::operator=(const SkClipStack& b) {
    if (this == &b) {
        return *this;
    }
    reset();

    fSaveCount = b.fSaveCount;
    SkDeque::F2BIter recIter(b.fDeque);
    for (const Element* element = static_cast<const Element*>(recIter.next());
         element != nullptr;
         element = static_cast<const Element*>(recIter.next())) {
        new (fDeque.push_back()) Element(*element);
    }

    return *this;
}

bool SkClipStack::operator==(const SkClipStack& b) const {
    if (this->getTopmostGenID() == b.getTopmostGenID()) {
        return true;
    }
    if (fSaveCount != b.fSaveCount ||
        fDeque.count() != b.fDeque.count()) {
        return false;
    }
    SkDeque::F2BIter myIter(fDeque);
    SkDeque::F2BIter bIter(b.fDeque);
    const Element* myElement = static_cast<const Element*>(myIter.next());
    const Element* bElement = static_cast<const Element*>(bIter.next());

    while (myElement != nullptr && bElement != nullptr) {
        if (*myElement != *bElement) {
            return false;
        }
        myElement = static_cast<const Element*>(myIter.next());
        bElement = static_cast<const Element*>(bIter.next());
    }
    return myElement == nullptr && bElement == nullptr;
}

void SkClipStack::reset() {
    while (!fDeque.empty()) {
        Element* element = static_cast<Element*>(fDeque.back());
        element->~Element();
        fDeque.pop_back();
    }
    fSaveCount = 0;
}

void SkClipStack::save() {
    fSaveCount += 1;
}

void SkClipStack::restore() {
    fSaveCount -= 1;
    restoreTo(fSaveCount);
}

void SkClipStack::restoreTo(int saveCount) {
    while (!fDeque.empty()) {
        Element* element = static_cast<Element*>(fDeque.back());
        if (element->fSaveCount <= saveCount) {
            break;
        }
        element->~Element();
        fDeque.pop_back();
    }
}

SkRect SkClipStack::bounds(const SkIRect& deviceBounds) const {
    SkRect r;
    SkClipStack::BoundsType bt;
    this->getBounds(&r, &bt);
    return skia_rs_clip_stack_compute_clipped_bounds(&r, static_cast<int32_t>(bt), &deviceBounds);
}

bool SkClipStack::isEmpty(const SkIRect& r) const {
    return this->bounds(r).isEmpty();
}

void SkClipStack::getBounds(SkRect* canvFiniteBound,
                            BoundsType* boundType,
                            bool* isIntersectionOfRects) const {
    SkASSERT(canvFiniteBound && boundType);

    const Element* element = static_cast<const Element*>(fDeque.back());
    if (nullptr == element) {
        canvFiniteBound->setEmpty();
        *boundType = kInsideOut_BoundsType;
        if (isIntersectionOfRects) {
            *isIntersectionOfRects = false;
        }
        return;
    }

    *canvFiniteBound = element->fFiniteBound;
    *boundType = element->fFiniteBoundType;
    if (isIntersectionOfRects) {
        *isIntersectionOfRects = element->fIsIntersectionOfRects;
    }
}

bool SkClipStack::internalQuickContains(const SkRect& rect) const {
    Iter iter(*this, Iter::kTop_IterStart);
    const Element* element = iter.prev();
    while (element != nullptr) {
        if (SkClipOp::kIntersect != element->getOp() && !element->isReplaceOp()) {
            return false;
        }
        if (element->isInverseFilled()) {
            if (SkRect::Intersects(element->getBounds(), rect)) {
                return false;
            }
        } else {
            if (!element->contains(rect)) {
                return false;
            }
        }
        if (element->isReplaceOp()) {
            break;
        }
        element = iter.prev();
    }
    return true;
}

bool SkClipStack::internalQuickContains(const SkRRect& rrect) const {
    Iter iter(*this, Iter::kTop_IterStart);
    const Element* element = iter.prev();
    while (element != nullptr) {
        if (SkClipOp::kIntersect != element->getOp() && !element->isReplaceOp()) {
            return false;
        }
        if (element->isInverseFilled()) {
            if (SkRect::Intersects(element->getBounds(), rrect.getBounds())) {
                return false;
            }
        } else {
            if (!element->contains(rrect)) {
                return false;
            }
        }
        if (element->isReplaceOp()) {
            break;
        }
        element = iter.prev();
    }
    return true;
}

void SkClipStack::pushElement(const Element& element) {
    SkDeque::Iter iter(fDeque, SkDeque::Iter::kBack_IterStart);
    Element* prior = static_cast<Element*>(iter.prev());

    const SkRect& newBounds = element.getBounds();
    const SkRect priorBounds = prior ? prior->getBounds() : SkRect::MakeEmpty();

    SkiaRsPushDecision decision = skia_rs_clip_stack_decide_push(
            prior != nullptr,
            prior ? static_cast<int32_t>(prior->fDeviceSpaceType) : 0,
            prior ? prior->fSaveCount : 0,
            prior ? static_cast<int32_t>(prior->fOp) : 0,
            prior ? prior->fIsReplace : false,
            prior ? prior->fDoAA : false,
            &priorBounds,
            fSaveCount,
            static_cast<int32_t>(element.fDeviceSpaceType),
            static_cast<int32_t>(element.fOp),
            element.fIsReplace,
            element.fDoAA,
            &newBounds);

    switch (decision.action) {
        case 0:  // NoOp
            SkDEBUGCODE(prior->checkEmpty();)
            return;
        case 1: {  // MergeShaders
            prior->fShader = SkShaders::Blend(SkBlendMode::kSrcIn,
                                              element.fShader, prior->fShader);
            Element* priorPrior = static_cast<Element*>(iter.prev());
            prior->updateBoundAndGenID(priorPrior);
            return;
        }
        case 2: {  // MergeRects
            prior->fDeviceSpaceRRect.setRect(decision.merged_rect);
            prior->fDoAA = element.isAA();
            Element* priorPrior = static_cast<Element*>(iter.prev());
            prior->updateBoundAndGenID(priorPrior);
            return;
        }
        case 3:  // SetPriorEmpty
            prior->setEmpty();
            return;
        case 4:  // ReplaceAndPush
            this->restoreTo(fSaveCount - 1);
            prior = static_cast<Element*>(fDeque.back());
            break;
        case 5:  // Push
        default:
            break;
    }

    Element* newElement = new (fDeque.push_back()) Element(element);
    newElement->updateBoundAndGenID(prior);
}

void SkClipStack::clipRRect(const SkRRect& rrect, const SkMatrix& matrix, SkClipOp op, bool doAA) {
    Element element(fSaveCount, rrect, matrix, op, doAA);
    this->pushElement(element);
}

void SkClipStack::clipRect(const SkRect& rect, const SkMatrix& matrix, SkClipOp op, bool doAA) {
    Element element(fSaveCount, rect, matrix, op, doAA);
    this->pushElement(element);
}

void SkClipStack::clipPath(const SkPath& path, const SkMatrix& matrix, SkClipOp op, bool doAA) {
    Element element(fSaveCount, path, matrix, op, doAA);
    this->pushElement(element);
}

void SkClipStack::clipShader(sk_sp<SkShader> shader) {
    Element element(fSaveCount, std::move(shader));
    this->pushElement(element);
}

void SkClipStack::replaceClip(const SkRect& rect, bool doAA) {
    Element element(fSaveCount, rect, doAA);
    this->pushElement(element);
}

void SkClipStack::clipEmpty() {
    Element* element = static_cast<Element*>(fDeque.back());
    if (element && element->canBeIntersectedInPlace(fSaveCount, SkClipOp::kIntersect)) {
        element->setEmpty();
    }
    new (fDeque.push_back()) Element(fSaveCount);
    static_cast<Element*>(fDeque.back())->fGenID = kEmptyGenID;
}

// ===========================================================================
// SkClipStack::Iter
// ===========================================================================

SkClipStack::Iter::Iter() : fStack(nullptr) {}

SkClipStack::Iter::Iter(const SkClipStack& stack, IterStart startLoc)
    : fStack(&stack) {
    this->reset(stack, startLoc);
}

const SkClipStack::Element* SkClipStack::Iter::next() {
    return static_cast<const Element*>(fIter.next());
}

const SkClipStack::Element* SkClipStack::Iter::prev() {
    return static_cast<const Element*>(fIter.prev());
}

const SkClipStack::Element* SkClipStack::Iter::skipToTopmost(SkClipOp op) {
    if (nullptr == fStack) {
        return nullptr;
    }

    fIter.reset(fStack->fDeque, SkDeque::Iter::kBack_IterStart);

    const Element* element = nullptr;
    for (element = static_cast<const Element*>(fIter.prev());
         element;
         element = static_cast<const Element*>(fIter.prev())) {
        if (op == element->fOp) {
            if (nullptr == fIter.next()) {
                fIter.reset(fStack->fDeque, SkDeque::Iter::kFront_IterStart);
            }
            break;
        }
    }

    if (nullptr == element) {
        fIter.reset(fStack->fDeque, SkDeque::Iter::kFront_IterStart);
    }

    return this->next();
}

void SkClipStack::Iter::reset(const SkClipStack& stack, IterStart startLoc) {
    fStack = &stack;
    fIter.reset(stack.fDeque, static_cast<SkDeque::Iter::IterStart>(startLoc));
}

void SkClipStack::getConservativeBounds(int offsetX,
                                        int offsetY,
                                        int maxWidth,
                                        int maxHeight,
                                        SkRect* devBounds,
                                        bool* isIntersectionOfRects) const {
    SkASSERT(devBounds);
    SkRect temp;
    BoundsType boundType;
    this->getBounds(&temp, &boundType, isIntersectionOfRects);
    *devBounds = skia_rs_clip_stack_compute_conservative_bounds(
            &temp, static_cast<int32_t>(boundType), offsetX, offsetY, maxWidth, maxHeight);
}

bool SkClipStack::isRRect(const SkRect& bounds, SkRRect* rrect, bool* aa) const {
    const Element* back = static_cast<const Element*>(fDeque.back());
    if (!back) {
        return false;
    }
    if (back->fIsIntersectionOfRects && back->fFiniteBoundType == BoundsType::kNormal_BoundsType) {
        rrect->setRect(back->fFiniteBound);
        *aa = back->isAA();
        return true;
    }

    if (back->getDeviceSpaceType() != Element::DeviceSpaceType::kRect &&
        back->getDeviceSpaceType() != Element::DeviceSpaceType::kRRect) {
        return false;
    }
    if (back->isReplaceOp()) {
        *rrect = back->asDeviceSpaceRRect();
        *aa = back->isAA();
        return true;
    }

    if (back->getOp() == SkClipOp::kIntersect) {
        SkRect backBounds;
        if (!backBounds.intersect(bounds, back->asDeviceSpaceRRect().rect())) {
            return false;
        }
        int cnt = fDeque.count();
        if (cnt > 17) {
            return false;
        }
        if (cnt > 1) {
            SkDeque::Iter iter(fDeque, SkDeque::Iter::kBack_IterStart);
            SkAssertResult(static_cast<const Element*>(iter.prev()) == back);
            while (const Element* prior = static_cast<const Element*>(iter.prev())) {
                if ((prior->getOp() != SkClipOp::kIntersect && !prior->isReplaceOp()) ||
                    !prior->contains(backBounds)) {
                    return false;
                }
                if (prior->isReplaceOp()) {
                    break;
                }
            }
        }
        *rrect = back->asDeviceSpaceRRect();
        *aa = back->isAA();
        return true;
    }
    return false;
}

uint32_t SkClipStack::GetNextGenID() {
    return skia_rs_clip_stack_next_gen_id();
}

uint32_t SkClipStack::getTopmostGenID() const {
    if (fDeque.empty()) {
        return skia_rs_clip_stack_compute_topmost_gen_id(true, 0, 0, nullptr, 0);
    }
    const Element* back = static_cast<const Element*>(fDeque.back());
    return skia_rs_clip_stack_compute_topmost_gen_id(
            false,
            static_cast<int32_t>(back->fDeviceSpaceType),
            static_cast<int32_t>(back->fFiniteBoundType),
            &back->fFiniteBound,
            back->getGenID());
}

#ifdef SK_DEBUG
void SkClipStack::Element::dump() const {
    static constexpr auto kTypeStrings = std::to_array<const char*>({
        "empty",
        "rect",
        "rrect",
        "path",
        "shader"
    });
    static_assert(0 == static_cast<int>(DeviceSpaceType::kEmpty), "enum mismatch");
    static_assert(1 == static_cast<int>(DeviceSpaceType::kRect), "enum mismatch");
    static_assert(2 == static_cast<int>(DeviceSpaceType::kRRect), "enum mismatch");
    static_assert(3 == static_cast<int>(DeviceSpaceType::kPath), "enum mismatch");
    static_assert(4 == static_cast<int>(DeviceSpaceType::kShader), "enum mismatch");
    static_assert(std::size(kTypeStrings) == kTypeCnt, "enum mismatch");

    const char* opName = this->isReplaceOp() ? "replace" :
            (fOp == SkClipOp::kDifference ? "difference" : "intersect");
    SkDebugf("Type: %s, Op: %s, AA: %s, Save Count: %d\n", kTypeStrings[(int)fDeviceSpaceType],
             opName, (fDoAA ? "yes" : "no"), fSaveCount);
    switch (fDeviceSpaceType) {
        case DeviceSpaceType::kEmpty:
            SkDebugf("\n");
            break;
        case DeviceSpaceType::kRect:
            this->getDeviceSpaceRect().dump();
            SkDebugf("\n");
            break;
        case DeviceSpaceType::kRRect:
            this->getDeviceSpaceRRect().dump();
            SkDebugf("\n");
            break;
        case DeviceSpaceType::kPath:
            this->getDeviceSpacePath().dump(nullptr, false);
            break;
        case DeviceSpaceType::kShader:
            break;
    }
}

void SkClipStack::dump() const {
    B2TIter iter(*this);
    const Element* e;
    while ((e = iter.next())) {
        e->dump();
        SkDebugf("\n");
    }
}
#endif
