/*
 * Safe Rust bridge for src/core/SkRTree.cpp and src/core/SkBBHFactory.cpp
 * (Layer 2.3 skia-rs-region).
 */

#include "include/core/SkBBHFactory.h"
#include "include/core/SkRect.h"
#include "include/private/SkAssert.h"
#include "include/private/SkDebug.h"
#include "src/core/SkRTree.h"

#include <cstddef>
#include <cstdint>
#include <vector>

static_assert(sizeof(SkRect) == 16);

extern "C" {
struct SkiaRsRTree;

struct SkiaRsRTreeBranch {
    uint32_t index;
    SkRect bounds;
};
static_assert(sizeof(SkiaRsRTreeBranch) == 20);

struct SkiaRsRTreeNode {
    uint16_t num_children;
    uint16_t level;
    SkiaRsRTreeBranch children[SkRTree::kMaxChildren];
};
static_assert(sizeof(SkiaRsRTreeNode) == 224);

int32_t skia_rs_rtree_count_nodes(int32_t branches);
SkiaRsRTree* skia_rs_rtree_build(const SkRect* bounds, int32_t n);
void skia_rs_rtree_info(
        const SkiaRsRTree* tree, int32_t* out_count, SkRect* out_root_bounds,
        uint32_t* out_root_node, const SkiaRsRTreeNode** out_nodes, size_t* out_nodes_len);
void skia_rs_rtree_free(SkiaRsRTree* tree);
void skia_rs_rtree_search_with(
        const SkRect* root_bounds, size_t root_node, size_t count, const SkRect* query,
        void (*read_node)(size_t, uint16_t*, uint16_t*, SkiaRsRTreeBranch*, void*),
        void (*on_hit)(int32_t, void*),
        void* ctx);
}  // extern "C"

SkRTree::SkRTree() : fCount(0) {}

void SkRTree::insert(const SkRect boundsArray[], int N) {
    SkASSERT(0 == fCount);

    SkiaRsRTree* rsTree = skia_rs_rtree_build(boundsArray, N);
    int32_t count = 0;
    SkRect rootBounds = SkRect::MakeEmpty();
    uint32_t rootNodeIdx = 0;
    const SkiaRsRTreeNode* rsNodes = nullptr;
    size_t nodesLen = 0;
    skia_rs_rtree_info(rsTree, &count, &rootBounds, &rootNodeIdx, &rsNodes, &nodesLen);

    fCount = count;
    if (fCount > 0) {
        fNodes.reserve(static_cast<size_t>(CountNodes(fCount)));
        fNodes.resize(nodesLen);
        for (size_t i = 0; i < nodesLen; ++i) {
            const SkiaRsRTreeNode& src = rsNodes[i];
            Node& dst = fNodes[i];
            dst.fNumChildren = src.num_children;
            dst.fLevel = src.level;
            for (uint16_t k = 0; k < src.num_children; ++k) {
                dst.fChildren[k].fBounds = src.children[k].bounds;
                if (src.level == 0) {
                    dst.fChildren[k].fOpIndex = static_cast<int>(src.children[k].index);
                } else {
                    dst.fChildren[k].fSubtree = &fNodes[src.children[k].index];
                }
            }
        }
        fRoot.fSubtree = &fNodes[rootNodeIdx];
        fRoot.fBounds = rootBounds;
    }

    skia_rs_rtree_free(rsTree);
}

SkRTree::Node* SkRTree::allocateNodeAtLevel(uint16_t level) {
    SkDEBUGCODE(Node* p = fNodes.data();)
    fNodes.push_back(Node{});
    Node& out = fNodes.back();
    SkASSERT(fNodes.data() == p);
    out.fNumChildren = 0;
    out.fLevel = level;
    return &out;
}

int SkRTree::CountNodes(int branches) {
    return skia_rs_rtree_count_nodes(branches);
}

SkRTree::Branch SkRTree::bulkLoad(std::vector<Branch>* branches, int level) {
    if (branches->size() == 1) {
        return (*branches)[0];
    }

    int remainder = static_cast<int>(branches->size()) % kMaxChildren;
    int newBranches = 0;

    if (remainder > 0) {
        if (remainder >= kMinChildren) {
            remainder = 0;
        } else {
            remainder = kMinChildren - remainder;
        }
    }

    int currentBranch = 0;
    while (currentBranch < static_cast<int>(branches->size())) {
        int incrementBy = kMaxChildren;
        if (remainder != 0) {
            if (remainder <= kMaxChildren - kMinChildren) {
                incrementBy -= remainder;
                remainder = 0;
            } else {
                incrementBy = kMinChildren;
                remainder -= kMaxChildren - kMinChildren;
            }
        }
        Node* n = allocateNodeAtLevel(static_cast<uint16_t>(level));
        n->fNumChildren = 1;
        n->fChildren[0] = (*branches)[currentBranch];
        Branch b;
        b.fBounds = (*branches)[currentBranch].fBounds;
        b.fSubtree = n;
        ++currentBranch;
        for (int k = 1; k < incrementBy && currentBranch < static_cast<int>(branches->size()); ++k) {
            b.fBounds.join((*branches)[currentBranch].fBounds);
            n->fChildren[k] = (*branches)[currentBranch];
            ++n->fNumChildren;
            ++currentBranch;
        }
        (*branches)[newBranches] = b;
        ++newBranches;
    }
    branches->resize(static_cast<size_t>(newBranches));
    return this->bulkLoad(branches, level + 1);
}

void SkRTree::search(const SkRect& query, std::vector<int>* results) const {
    if (fCount == 0) {
        return;
    }
    struct SearchCtx {
        const Node* base;
        std::vector<int>* results;
    };
    SearchCtx ctx{fNodes.data(), results};

    auto readNode = [](size_t idx, uint16_t* outLevel, uint16_t* outNum,
                       SkiaRsRTreeBranch* outChildren, void* rawCtx) {
        const auto* c = static_cast<const SearchCtx*>(rawCtx);
        const Node& node = c->base[idx];
        *outLevel = node.fLevel;
        *outNum = node.fNumChildren;
        for (uint16_t i = 0; i < node.fNumChildren; ++i) {
            outChildren[i].bounds = node.fChildren[i].fBounds;
            if (node.fLevel == 0) {
                outChildren[i].index = static_cast<uint32_t>(node.fChildren[i].fOpIndex);
            } else {
                outChildren[i].index = static_cast<uint32_t>(node.fChildren[i].fSubtree - c->base);
            }
        }
    };
    auto onHit = [](int32_t opIdx, void* rawCtx) {
        static_cast<SearchCtx*>(rawCtx)->results->push_back(opIdx);
    };

    const size_t rootIdx = static_cast<size_t>(fRoot.fSubtree - fNodes.data());
    skia_rs_rtree_search_with(
            &fRoot.fBounds, rootIdx, static_cast<size_t>(fCount), &query,
            readNode, onHit, &ctx);
}

void SkRTree::search(Node* node, const SkRect& query, std::vector<int>* results) const {
    if (!node) {
        return;
    }
    for (int i = 0; i < node->fNumChildren; ++i) {
        if (SkRect::Intersects(node->fChildren[i].fBounds, query)) {
            if (0 == node->fLevel) {
                results->push_back(node->fChildren[i].fOpIndex);
            } else {
                this->search(node->fChildren[i].fSubtree, query, results);
            }
        }
    }
}

size_t SkRTree::bytesUsed() const {
    size_t byteCount = sizeof(SkRTree);
    byteCount += fNodes.capacity() * sizeof(Node);
    return byteCount;
}

// ===========================================================================
// SkBBHFactory.cpp symbols
// ===========================================================================

sk_sp<SkBBoxHierarchy> SkRTreeFactory::operator()() const {
    return sk_make_sp<SkRTree>();
}

void SkBBoxHierarchy::insert(const SkRect rects[], const Metadata[], int N) {
    this->insert(rects, N);
}
