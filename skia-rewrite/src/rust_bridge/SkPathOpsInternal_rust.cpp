/*
 * Safe Rust bridge for Phase 1 Layer 3.1 skia-rs-pathops internal symbols
 * referenced by Skia's PathOps unit tests (PathOpsAngleTest, PathOpsDebug, etc.):
 *   SkOpAngle, SkOpSegment, SkOpSpan/SkOpPtT, SkPathOpsDebug.
 */

#include "include/core/SkPath.h"
#include "include/core/SkPathTypes.h"
#include "include/core/SkPoint.h"
#include "include/core/SkScalar.h"
#include "include/core/SkTypes.h"
#include "include/pathops/SkPathOps.h"
#include "include/private/SkDebug.h"
#include "src/core/SkPathPriv.h"
#include "src/pathops/SkOpAngle.h"
#include "src/pathops/SkOpContour.h"
#include "src/pathops/SkOpSegment.h"
#include "src/pathops/SkOpSpan.h"
#include "src/pathops/SkPathOpsConic.h"
#include "src/pathops/SkPathOpsCubic.h"
#include "src/pathops/SkPathOpsDebug.h"
#include "src/pathops/SkPathOpsQuad.h"
#include "src/pathops/SkPathOpsRect.h"
#include "src/pathops/SkPathOpsTypes.h"

#include <array>
#include <cstdint>
#include <cstring>

struct FfiAngleDesc {
    SkPoint pts[4];
    float weight;
    uint8_t verb;
    double start_t;
    double end_t;
};

static_assert(sizeof(SkPoint) == 8);

extern "C" {
int skia_rs_pathops_angle_orderable(const FfiAngleDesc* a, const FfiAngleDesc* b);
bool skia_rs_pathops_angle_after(const FfiAngleDesc* a, const FfiAngleDesc* b,
                                 const FfiAngleDesc* b_next);
int skia_rs_pathops_angle_line_on_one_side(const FfiAngleDesc* a, const FfiAngleDesc* b,
                                           bool use_original);
int skia_rs_pathops_angle_convex_hull_overlaps(const FfiAngleDesc* a, const FfiAngleDesc* b);
bool skia_rs_pathops_angle_ends_intersect(const FfiAngleDesc* a, const FfiAngleDesc* b);
SkPoint skia_rs_pathops_curve_point_at_t(const SkPoint* pts, float weight, uint8_t verb, double t);
bool skia_rs_pathops_seg_match(const SkPoint* pts, float weight, uint8_t verb, bool same_seg,
                               double base_t, SkPoint base_pt, double test_t, SkPoint test_pt);
}  // extern "C"

namespace {

FfiAngleDesc make_angle_desc(const SkOpAngle* angle) {
    FfiAngleDesc desc{};
    const SkOpSegment* seg = angle->segment();
    SkPath::Verb verb = seg->verb();
    int ptCount = SkPathOpsVerbToPoints(verb) + 1;
    for (int i = 0; i < ptCount && i < 4; ++i) {
        desc.pts[i] = seg->pts()[i];
    }
    desc.weight = seg->weight();
    desc.verb = static_cast<uint8_t>(verb);
    desc.start_t = angle->start()->t();
    desc.end_t = angle->end()->t();
    return desc;
}

}  // namespace

// ============================================================================
// SkOpAngle methods (backed by safe Rust OpState)
// ============================================================================

SkOpSegment* SkOpAngle::segment() const {
    return fStart->segment();
}

void SkOpAngle::set(SkOpSpanBase* start, SkOpSpanBase* end) {
    fStart = start;
    fEnd = end;
    fComputedEnd = end;
    fNext = nullptr;
    fLastMarked = nullptr;
    fSectorMask = 0;
    fSectorStart = 0;
    fSectorEnd = 0;
    fUnorderable = false;
    fComputeSector = false;
    fComputedSector = false;
    fCheckCoincidence = false;
    fTangentsAmbiguous = false;
}

int SkOpAngle::orderable(SkOpAngle* rh) {
    FfiAngleDesc a = make_angle_desc(this);
    FfiAngleDesc b = make_angle_desc(rh);
    return skia_rs_pathops_angle_orderable(&a, &b);
}

bool SkOpAngle::after(SkOpAngle* test) {
    FfiAngleDesc a = make_angle_desc(this);
    FfiAngleDesc b = make_angle_desc(test);
    FfiAngleDesc bNext{};
    const FfiAngleDesc* bNextPtr = nullptr;
    if (test->fNext) {
        bNext = make_angle_desc(test->fNext);
        bNextPtr = &bNext;
    }
    return skia_rs_pathops_angle_after(&a, &b, bNextPtr);
}

int SkOpAngle::lineOnOneSide(const SkOpAngle* test, bool useOriginal) {
    FfiAngleDesc a = make_angle_desc(this);
    FfiAngleDesc b = make_angle_desc(test);
    return skia_rs_pathops_angle_line_on_one_side(&a, &b, useOriginal);
}

int SkOpAngle::convexHullOverlaps(const SkOpAngle* rh) {
    FfiAngleDesc a = make_angle_desc(this);
    FfiAngleDesc b = make_angle_desc(rh);
    return skia_rs_pathops_angle_convex_hull_overlaps(&a, &b);
}

bool SkOpAngle::endsIntersect(SkOpAngle* rh) {
    FfiAngleDesc a = make_angle_desc(this);
    FfiAngleDesc b = make_angle_desc(rh);
    return skia_rs_pathops_angle_ends_intersect(&a, &b);
}

// ============================================================================
// SkOpSegment & SkOpSpanBase / SkOpPtT methods used by tests
// ============================================================================

SkOpGlobalState* SkOpSegment::globalState() const {
    return contour()->globalState();
}

bool SkOpSegment::isXor() const {
    return fContour->isXor();
}

bool SkOpSegment::operand() const {
    return fContour->operand();
}

bool SkOpSegment::oppXor() const {
    return fContour->oppXor();
}

void SkOpSegment::init(SkPoint pts[], SkScalar weight, SkOpContour* contour, SkPath::Verb verb) {
    fContour = contour;
    fNext = nullptr;
    fPts = pts;
    fWeight = weight;
    fVerb = verb;
    fCount = 0;
    fDoneCount = 0;
    fVisited = false;
    SkOpSpan* zeroSpan = &fHead;
    zeroSpan->init(this, nullptr, 0, fPts[0]);
    SkOpSpanBase* oneSpan = &fTail;
    zeroSpan->setNext(oneSpan);
    oneSpan->initBase(this, zeroSpan, 1, fPts[SkPathOpsVerbToPoints(fVerb)]);
    SkDEBUGCODE(fID = globalState()->nextSegmentID());
}

bool SkOpSegment::match(const SkOpPtT* base, const SkOpSegment* testParent, double testT,
                        const SkPoint& testPt) const {
    SkPoint copyPts[4] = {};
    int ptCount = SkPathOpsVerbToPoints(fVerb) + 1;
    for (int i = 0; i < ptCount && i < 4; ++i) {
        copyPts[i] = fPts[i];
    }
    return skia_rs_pathops_seg_match(copyPts, fWeight, static_cast<uint8_t>(fVerb),
                                     this == testParent, base->fT, base->fPt, testT, testPt);
}

SkOpPtT* SkOpSegment::addT(double t, const SkPoint& pt) {
    SkOpSpanBase* spanBase = &fHead;
    do {
        SkOpPtT* result = spanBase->ptT();
        if (t == result->fT || (!zero_or_one(t) && this->match(result, this, t, pt))) {
            spanBase->bumpSpanAdds();
            return result;
        }
        if (t < result->fT) {
            SkOpSpan* prev = result->span()->prev();
            if (!prev) {
                return nullptr;
            }
            SkOpSpan* span = this->insert(prev);
            span->init(this, prev, t, pt);
            span->bumpSpanAdds();
            return span->ptT();
        }
        if (spanBase == &fTail) {
            return nullptr;
        }
    } while ((spanBase = spanBase->upCast()->next()));
    return nullptr;
}

SkOpPtT* SkOpSegment::addT(double t) {
    SkPoint pt = skia_rs_pathops_curve_point_at_t(fPts, fWeight, static_cast<uint8_t>(fVerb), t);
    return addT(t, pt);
}

SkOpAngle* SkOpSegment::debugLastAngle() {
    SkOpAngle* result = nullptr;
    SkOpSpan* span = this->head();
    do {
        if (span->toAngle()) {
            result = span->toAngle();
        }
    } while ((span = span->next()->upCastable()));
    return result;
}

void SkOpPtT::init(SkOpSpanBase* span, double t, const SkPoint& pt, bool duplicate) {
    fT = t;
    fPt = pt;
    fSpan = span;
    fNext = this;
    fDuplicatePt = duplicate;
    fDeleted = false;
    fCoincident = false;
    SkDEBUGCODE(fID = span->globalState()->nextPtTID());
}

void SkOpSpanBase::initBase(SkOpSegment* segment, SkOpSpan* prev, double t, const SkPoint& pt) {
    fSegment = segment;
    fPtT.init(this, t, pt, false);
    fCoinEnd = this;
    fFromAngle = nullptr;
    fPrev = prev;
    fSpanAdds = 0;
    fAligned = true;
    fChased = false;
    SkDEBUGCODE(fCount = 1);
    SkDEBUGCODE(fID = globalState()->nextSpanID());
    SkDEBUGCODE(fDebugDeleted = false);
}

void SkOpSpan::init(SkOpSegment* segment, SkOpSpan* prev, double t, const SkPoint& pt) {
    SkASSERT(t != 1);
    initBase(segment, prev, t, pt);
    fCoincident = this;
    fToAngle = nullptr;
    fWindSum = fOppSum = SK_MinS32;
    fWindValue = 1;
    fOppValue = 0;
    fTopTTry = 0;
    fChased = fDone = false;
    segment->bumpCount();
    fAlreadyAdded = false;
}

SkOpContour* SkOpSpanBase::contour() const {
    return segment()->contour();
}

SkOpContour* SkOpPtT::contour() const {
    return segment()->contour();
}

const SkOpSegment* SkOpPtT::segment() const {
    return span()->segment();
}

SkOpSegment* SkOpPtT::segment() {
    return span()->segment();
}

// ============================================================================
// SkPathOpsDebug symbols referenced by tests
// ============================================================================

bool SkPathOpsDebug::gVeryVerbose = false;

static constexpr auto kDebugOpStrs = std::to_array<const char*>({
    "kDifference_SkPathOp",
    "kIntersect_SkPathOp",
    "kUnion_SkPathOp",
    "kXOR_SkPathOp",
    "kReverseDifference_SkPathOp",
});

const char* SkPathOpsDebug::OpStr(SkPathOp op) {
    return kDebugOpStrs[op];
}

void SkPathOpsDebug::ShowOnePath(const SkPath& path, const char* name, bool includeDeclaration) {
    static constexpr auto kFillTypeStr = std::to_array<const char*>({
        "kWinding",
        "kEvenOdd",
        "kInverseWinding",
        "kInverseEvenOdd",
    });
    SkPathFillType fillType = path.getFillType();
    if (includeDeclaration) {
        SkDebugf("    SkPath %s;\n", name);
    }
    SkDebugf("    %s.setFillType(SkPathFillType::%s);\n", name,
             kFillTypeStr[static_cast<int>(fillType)]);
    for (auto [verb, pts, w] : SkPathPriv::Iterate(path)) {
        switch (verb) {
            case SkPathVerb::kMove:
                SkDebugf("    %s.moveTo(%1.9gf, %1.9gf);\n", name, pts[0].fX, pts[0].fY);
                break;
            case SkPathVerb::kLine:
                SkDebugf("    %s.lineTo(%1.9gf, %1.9gf);\n", name, pts[1].fX, pts[1].fY);
                break;
            case SkPathVerb::kQuad:
                SkDebugf("    %s.quadTo(%1.9gf, %1.9gf, %1.9gf, %1.9gf);\n",
                         name, pts[1].fX, pts[1].fY, pts[2].fX, pts[2].fY);
                break;
            case SkPathVerb::kConic:
                SkDebugf("    %s.conicTo(%1.9gf, %1.9gf, %1.9gf, %1.9gf, %1.9gf);\n",
                         name, pts[1].fX, pts[1].fY, pts[2].fX, pts[2].fY, *w);
                break;
            case SkPathVerb::kCubic:
                SkDebugf("    %s.cubicTo(%1.9gf, %1.9gf, %1.9gf, %1.9gf, %1.9gf, %1.9gf);\n",
                         name, pts[1].fX, pts[1].fY, pts[2].fX, pts[2].fY, pts[3].fX, pts[3].fY);
                break;
            case SkPathVerb::kClose:
                SkDebugf("    %s.close();\n", name);
                break;
        }
    }
}

bool SkOpGlobalState::DebugRunFail() {
    return false;
}

SkDCubic SkDQuad::debugToCubic() const {
    SkDCubic cubic;
    cubic[0] = fPts[0];
    cubic[2] = fPts[1];
    cubic[3] = fPts[2];
    cubic[1].fX = (cubic[0].fX + cubic[2].fX * 2) / 3;
    cubic[1].fY = (cubic[0].fY + cubic[2].fY * 2) / 3;
    cubic[2].fX = (cubic[3].fX + cubic[2].fX * 2) / 3;
    cubic[2].fY = (cubic[3].fY + cubic[2].fY * 2) / 3;
    return cubic;
}

void SkDQuad::debugSet(const SkDPoint* pts) {
    memcpy(fPts, pts, sizeof(fPts));
    SkDEBUGCODE(fDebugGlobalState = nullptr);
}

void SkDCubic::debugSet(const SkDPoint* pts) {
    memcpy(fPts, pts, sizeof(fPts));
    SkDEBUGCODE(fDebugGlobalState = nullptr);
}

void SkDConic::debugSet(const SkDPoint* pts, SkScalar weight) {
    fPts.debugSet(pts);
    fWeight = weight;
}

void SkDRect::debugInit() {
    fLeft = fTop = fRight = fBottom = SK_ScalarNaN;
}

int SkOpPtT::debugLoopLimit(bool report) const {
    int loop = 0;
    const SkOpPtT* next = this;
    do {
        for (int check = 1; check < loop - 1; ++check) {
            const SkOpPtT* checkPtT = this->fNext;
            const SkOpPtT* innerPtT = checkPtT;
            for (int inner = check + 1; inner < loop; ++inner) {
                innerPtT = innerPtT->fNext;
                if (checkPtT == innerPtT) {
                    if (report) {
                        SkDebugf("*** bad ptT loop ***\n");
                    }
                    return loop;
                }
            }
        }
        if (++loop > 1000) {
            SkDebugf("*** loop count exceeds 1000 ***\n");
            return 1000;
        }
    } while ((next = next->fNext) && next != this);
    return 0;
}

bool SkOpSpan::debugCoinLoopCheck() const {
    int loop = 0;
    const SkOpSpan* next = this;
    SkOpSpan* nextCoin;
    do {
        nextCoin = next->fCoincident;
        for (int check = 1; check < loop - 1; ++check) {
            const SkOpSpan* checkCoin = this->fCoincident;
            const SkOpSpan* innerCoin = checkCoin;
            for (int inner = check + 1; inner < loop; ++inner) {
                innerCoin = innerCoin->fCoincident;
                if (checkCoin == innerCoin) {
                    SkDebugf("*** bad coincident loop ***\n");
                    return false;
                }
            }
        }
        ++loop;
    } while ((next = nextCoin) && next != this);
    return true;
}
