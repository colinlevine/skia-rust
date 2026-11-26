/*
 * Safe Rust bridge for:
 *   src/core/SkDocument.cpp
 *   src/core/SkCapabilities.cpp
 *   src/core/SkGraphics.cpp
 *   src/core/SkGlobalInitialization_core.cpp
 *   src/core/SkMesh.cpp
 * (Phase 1 Layer 5.3 skia-rs-canvas, GN target :core).
 */

#include "include/core/SkAlphaType.h"
#include "include/core/SkCanvas.h"
#include "include/core/SkCapabilities.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkData.h"
#include "include/core/SkDocument.h"
#include "include/core/SkFlattenable.h"
#include "include/core/SkGraphics.h"
#include "include/core/SkMesh.h"
#include "include/core/SkRect.h"
#include "include/private/SkAlign.h"
#include "include/private/SkAssert.h"
#include "include/private/SkMath.h"
#include "include/private/SkOnce.h"
#include "include/private/SkSLSampleUsage.h"
#include "include/private/SkTArray.h"
#include "include/private/SkTo.h"
#include "src/core/SkContextPriv.h"
#include "src/core/SkImageFilter_Base.h"
#include "src/core/SkMeshPriv.h"
#include "src/core/SkResourceCache.h"
#include "src/core/SkRuntimeEffectPriv.h"
#include "src/core/SkStrikeCache.h"
#include "src/core/SkTypefaceCache.h"
#include "src/sksl/SkSLAnalysis.h"
#include "src/sksl/SkSLBuiltinTypes.h"
#include "src/sksl/SkSLCompiler.h"
#include "src/sksl/SkSLContext.h"
#include "src/sksl/SkSLProgramKind.h"
#include "src/sksl/SkSLProgramSettings.h"
#include "src/sksl/SkSLUtil.h"
#include "src/sksl/analysis/SkSLProgramVisitor.h"
#include "src/sksl/ir/SkSLExpression.h"
#include "src/sksl/ir/SkSLFieldAccess.h"
#include "src/sksl/ir/SkSLFunctionDeclaration.h"
#include "src/sksl/ir/SkSLFunctionDefinition.h"
#include "src/sksl/ir/SkSLModifierFlags.h"
#include "src/sksl/ir/SkSLProgram.h"
#include "src/sksl/ir/SkSLProgramElement.h"
#include "src/sksl/ir/SkSLReturnStatement.h"
#include "src/sksl/ir/SkSLStatement.h"
#include "src/sksl/ir/SkSLStructDefinition.h"
#include "src/sksl/ir/SkSLType.h"
#include "src/sksl/ir/SkSLVarDeclarations.h"
#include "src/sksl/ir/SkSLVariable.h"
#include "src/sksl/ir/SkSLVariableReference.h"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <string>
#include <tuple>
#include <utility>

using namespace skia_private;

namespace {

struct SkiaRsMeshAttrDesc {
    uint32_t attr_type;
    size_t offset;
};

struct SkiaRsDocumentTrimResult {
    bool valid;
    SkRect inner;
};

static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkiaRsMeshAttrDesc) == 16);
static_assert(sizeof(SkiaRsDocumentTrimResult) == 20);

extern "C" {
bool skia_rs_canvas_mesh_check_name(const uint8_t* name, size_t len);
int32_t skia_rs_canvas_mesh_check_layout(
        const SkiaRsMeshAttrDesc* attrs, size_t count, size_t stride);
size_t skia_rs_canvas_mesh_uniform_size(
        bool has_uniforms, size_t last_offset, size_t last_size);
uint32_t skia_rs_canvas_mesh_spec_hash(
        const uint8_t* vs, size_t vs_len,
        const uint8_t* fs, size_t fs_len,
        const SkiaRsMeshAttrDesc* attrs, size_t attr_count,
        size_t stride, uint64_t cs_hash, uint32_t alpha_type);
int32_t skia_rs_canvas_mesh_validate_buffers(
        size_t stride, size_t vcount, size_t voffset, size_t vb_size,
        size_t spec_uniform_size, bool has_uniforms, size_t uniforms_size,
        bool has_ib, size_t ib_size, size_t icount, size_t ioffset, int32_t mode);
bool skia_rs_canvas_mesh_check_update(
        bool has_data, size_t offset, size_t size, size_t buffer_size);
bool skia_rs_canvas_doc_can_begin_page(float width, float height, int32_t state);
SkiaRsDocumentTrimResult skia_rs_canvas_doc_trim_content(float width, float height, SkRect content);
}

}  // namespace

// ============================================================================
// SkDocument.cpp
// ============================================================================

SkDocument::SkDocument(SkWStream* stream) : fStream(stream), fState(kBetweenPages_State) {}

SkDocument::~SkDocument() {
    this->close();
}

static SkCanvas* trim_doc_canvas(SkCanvas* canvas, SkScalar width, SkScalar height,
                                 const SkRect* content) {
    if (content && canvas) {
        SkiaRsDocumentTrimResult res = skia_rs_canvas_doc_trim_content(width, height, *content);
        if (!res.valid) {
            return nullptr;
        }
        canvas->clipRect(res.inner);
        canvas->translate(res.inner.x(), res.inner.y());
    }
    return canvas;
}

SkCanvas* SkDocument::beginPage(SkScalar width, SkScalar height, const SkRect* content) {
    if (!skia_rs_canvas_doc_can_begin_page(width, height, static_cast<int32_t>(fState))) {
        return nullptr;
    }
    if (kInPage_State == fState) {
        this->endPage();
    }
    SkASSERT(kBetweenPages_State == fState);
    fState = kInPage_State;
    return trim_doc_canvas(this->onBeginPage(width, height), width, height, content);
}

void SkDocument::endPage() {
    if (kInPage_State == fState) {
        fState = kBetweenPages_State;
        this->onEndPage();
    }
}

void SkDocument::close() {
    for (;;) {
        switch (fState) {
            case kBetweenPages_State: {
                fState = kClosed_State;
                this->onClose(fStream);
                fStream = nullptr;
                return;
            }
            case kInPage_State:
                this->endPage();
                break;
            case kClosed_State:
                return;
        }
    }
}

void SkDocument::abort() {
    this->onAbort();
    fState = kClosed_State;
    fStream = nullptr;
}

// ============================================================================
// SkCapabilities.cpp
// ============================================================================

sk_sp<const SkCapabilities> SkCapabilities::RasterBackend() {
    static SkCapabilities* sCaps = []() {
        SkCapabilities* caps = new SkCapabilities;
        caps->fSkSLVersion = SkSL::Version::k100;
        return caps;
    }();
    return sk_ref_sp(sCaps);
}

void SkCapabilities::initSkCaps(const SkSL::ShaderCaps* shaderCaps) {
    this->fSkSLVersion = shaderCaps->supportedSkSLVerion();
}

// ============================================================================
// SkGraphics.cpp
// ============================================================================

void SkGraphics::Init() {
    SkContexts::GetGlobalSkContext(/*enableRuntimeOpts=*/true);
}

void SkGraphics::DumpMemoryStatistics(SkTraceMemoryDump* dump) {
    SkResourceCache::DumpMemoryStatistics(dump);
    SkStrikeCache::DumpMemoryStatistics(dump);
}

void SkGraphics::PurgeAllCaches() {
    SkGraphics::PurgeFontCache();
    SkGraphics::PurgeResourceCache();
    SkImageFilter_Base::PurgeCache();
}

size_t SkGraphics::GetFontCacheLimit() {
#if defined(SK_ENABLE_THREADLOCAL_STRIKECACHE)
    return SkStrikeCache::GlobalStrikeCache()->getCacheSizeLimit();
#else
    return SkContexts::GetGlobalSkContext()->fontCacheLimit();
#endif
}

size_t SkGraphics::SetFontCacheLimit(size_t bytes) {
#if defined(SK_ENABLE_THREADLOCAL_STRIKECACHE)
    return SkStrikeCache::GlobalStrikeCache()->setCacheSizeLimit(bytes);
#else
    return SkContexts::GetGlobalSkContext()->setFontCacheLimit(bytes);
#endif
}

size_t SkGraphics::GetFontCacheUsed() {
#if defined(SK_ENABLE_THREADLOCAL_STRIKECACHE)
    return SkStrikeCache::GlobalStrikeCache()->getTotalMemoryUsed();
#else
    return SkContexts::GetGlobalSkContext()->fontCacheUsed();
#endif
}

int SkGraphics::GetFontCacheCountLimit() {
#if defined(SK_ENABLE_THREADLOCAL_STRIKECACHE)
    return SkStrikeCache::GlobalStrikeCache()->getCacheCountLimit();
#else
    return SkContexts::GetGlobalSkContext()->fontCacheCountLimit();
#endif
}

int SkGraphics::SetFontCacheCountLimit(int count) {
#if defined(SK_ENABLE_THREADLOCAL_STRIKECACHE)
    return SkStrikeCache::GlobalStrikeCache()->setCacheCountLimit(count);
#else
    return SkContexts::GetGlobalSkContext()->setFontCacheCountLimit(count);
#endif
}

int SkGraphics::GetFontCacheCountUsed() {
#if defined(SK_ENABLE_THREADLOCAL_STRIKECACHE)
    return SkStrikeCache::GlobalStrikeCache()->getCacheCountUsed();
#else
    return SkContexts::GetGlobalSkContext()->fontCacheCountUsed();
#endif
}

void SkGraphics::PurgeFontCache() {
#if defined(SK_ENABLE_THREADLOCAL_STRIKECACHE)
    SkStrikeCache::GlobalStrikeCache()->purgeAll();
#else
    SkContexts::GetGlobalSkContext()->purgeFontCache();
#endif
    SkTypefaceCache::PurgeAll();
}

void SkGraphics::PurgePinnedFontCache() {
#if defined(SK_ENABLE_THREADLOCAL_STRIKECACHE)
    SkStrikeCache::GlobalStrikeCache()->purgePinned();
#else
    SkContexts::GetGlobalSkContext()->purgePinnedFontCache();
#endif
}

size_t SkGraphics::GetResourceCacheTotalBytesUsed() {
    return SkContexts::GetGlobalSkContext()->resourceCacheTotalBytesUsed();
}

size_t SkGraphics::GetResourceCacheTotalByteLimit() {
    return SkContexts::GetGlobalSkContext()->resourceCacheTotalByteLimit();
}

size_t SkGraphics::SetResourceCacheTotalByteLimit(size_t newLimit) {
    return SkContexts::GetGlobalSkContext()->setResourceCacheTotalByteLimit(newLimit);
}

size_t SkGraphics::GetResourceCacheSingleAllocationByteLimit() {
    return SkContexts::GetGlobalSkContext()->resourceCacheSingleAllocationByteLimit();
}

size_t SkGraphics::SetResourceCacheSingleAllocationByteLimit(size_t newLimit) {
    return SkContexts::GetGlobalSkContext()->setResourceCacheSingleAllocationByteLimit(newLimit);
}

void SkGraphics::PurgeResourceCache() {
    SkImageFilter_Base::PurgeCache();
    return SkContexts::GetGlobalSkContext()->purgeResourceCache();
}

int SkGraphics::GetTypefaceCacheCountLimit() {
    return SkContexts::GetGlobalSkContext()->typefaceCacheCountLimit();
}

int SkGraphics::SetTypefaceCacheCountLimit(int count) {
    return SkContexts::GetGlobalSkContext()->setTypefaceCacheCountLimit(count);
}

SkGraphics::OpenTypeSVGDecoderFactory SkGraphics::SetOpenTypeSVGDecoderFactory(
        OpenTypeSVGDecoderFactory svgDecoderFactory) {
    SkSharedState* shrdState = SkContexts::GetGlobalSkContext()->priv().sharedState();
    return shrdState->setSVGDecoderFactory(svgDecoderFactory);
}

SkGraphics::OpenTypeSVGDecoderFactory SkGraphics::GetOpenTypeSVGDecoderFactory() {
    SkSharedState* shrdState = SkContexts::GetGlobalSkContext()->priv().sharedState();
    return shrdState->svgDecoderFactory();
}

// ============================================================================
// SkGlobalInitialization_core.cpp
// ============================================================================

void SkFlattenable::RegisterFlattenablesIfNeeded() {
    static SkOnce once;
    once([] {
        SkFlattenable::PrivateInitializer::InitEffects();
        SkFlattenable::PrivateInitializer::InitImageFilters();
        SkFlattenable::Finalize();
    });
}

// ============================================================================
// SkMesh.cpp
// ============================================================================

using Attribute = SkMeshSpecification::Attribute;
using Varying   = SkMeshSpecification::Varying;
using IndexBuffer  = SkMesh::IndexBuffer;
using VertexBuffer = SkMesh::VertexBuffer;
using Uniform = SkMeshSpecification::Uniform;
using Child = SkMeshSpecification::Child;
using ColorType = SkMeshSpecificationPriv::ColorType;

#define RETURN_FAILURE(...) return Result{nullptr, SkStringPrintf(__VA_ARGS__)}
#define RETURN_ERROR(...) return std::make_tuple(false, SkStringPrintf(__VA_ARGS__))
#define RETURN_SUCCESS return std::make_tuple(true, SkString{})

static std::vector<Uniform>::iterator find_uniform(std::vector<Uniform>& uniforms,
                                                   std::string_view name) {
    return std::find_if(uniforms.begin(), uniforms.end(),
                        [name](const SkMeshSpecification::Uniform& u) { return u.name == name; });
}

static std::tuple<bool, SkString>
gather_uniforms_and_check_for_main(const SkSL::Program& program,
                                   std::vector<Uniform>* uniforms,
                                   std::vector<Child>* children,
                                   SkMeshSpecification::Uniform::Flags stage,
                                   size_t* offset) {
    bool foundMain = false;
    for (const SkSL::ProgramElement* elem : program.elements()) {
        if (elem->is<SkSL::FunctionDefinition>()) {
            const SkSL::FunctionDefinition& defn = elem->as<SkSL::FunctionDefinition>();
            const SkSL::FunctionDeclaration& decl = defn.declaration();
            if (decl.isMain()) {
                foundMain = true;
            }
        } else if (elem->is<SkSL::GlobalVarDeclaration>()) {
            const SkSL::GlobalVarDeclaration& global = elem->as<SkSL::GlobalVarDeclaration>();
            const SkSL::VarDeclaration& varDecl = global.declaration()->as<SkSL::VarDeclaration>();
            const SkSL::Variable& var = *varDecl.var();
            if (var.modifierFlags().isUniform()) {
                if (var.type().isEffectChild()) {
                    children->push_back(SkRuntimeEffectPriv::VarAsChild(var, children->size()));
                } else {
                    auto iter = find_uniform(*uniforms, var.name());
                    const auto& context = *program.fContext;
                    if (iter == uniforms->end()) {
                        uniforms->push_back(
                                SkRuntimeEffectPriv::VarAsUniform(var, context, offset));
                        uniforms->back().flags |= stage;
                    } else {
                        size_t ignoredOffset = 0;
                        auto uniform =
                                SkRuntimeEffectPriv::VarAsUniform(var, context, &ignoredOffset);
                        if (uniform.isArray() != iter->isArray() ||
                            uniform.type      != iter->type      ||
                            uniform.count     != iter->count) {
                            return {false,
                                    SkStringPrintf("Uniform %.*s declared with different types"
                                                   " in vertex and fragment shaders.",
                                                   (int)var.name().size(), var.name().data())};
                        }
                        if (uniform.isColor() != iter->isColor()) {
                            return {false,
                                    SkStringPrintf("Uniform %.*s declared with different color"
                                                   " layout in vertex and fragment shaders.",
                                                   (int)var.name().size(), var.name().data())};
                        }
                        (*iter).flags |= stage;
                    }
                }
            }
        }
    }
    if (!foundMain) {
        return {false, SkString("No main function found.")};
    }
    return {true, {}};
}

// Non-static for 100% strong-symbol parity with SkMesh.cpp.
ColorType get_fs_color_type(const SkSL::Program& fsProgram) {
    for (const SkSL::ProgramElement* elem : fsProgram.elements()) {
        if (elem->is<SkSL::FunctionDefinition>()) {
            const SkSL::FunctionDefinition& defn = elem->as<SkSL::FunctionDefinition>();
            const SkSL::FunctionDeclaration& decl = defn.declaration();
            if (decl.isMain()) {
                SkASSERT(decl.parameters().size() == 1 || decl.parameters().size() == 2);
                if (decl.parameters().size() == 1) {
                    return ColorType::kNone;
                }
                const SkSL::Type& paramType = decl.parameters()[1]->type();
                SkASSERT(paramType.matches(*fsProgram.fContext->fTypes.fHalf4) ||
                         paramType.matches(*fsProgram.fContext->fTypes.fFloat4));
                return paramType.matches(*fsProgram.fContext->fTypes.fHalf4) ? ColorType::kHalf4
                                                                             : ColorType::kFloat4;
            }
        }
    }
    SkUNREACHABLE;
}

static bool check_name(const SkString& name) {
    return skia_rs_canvas_mesh_check_name(
            reinterpret_cast<const uint8_t*>(name.data()), name.size());
}

static const char* attribute_type_string(Attribute::Type type) {
    switch (type) {
        case Attribute::Type::kFloat:         return "float";
        case Attribute::Type::kFloat2:        return "float2";
        case Attribute::Type::kFloat3:        return "float3";
        case Attribute::Type::kFloat4:        return "float4";
        case Attribute::Type::kUByte4_unorm:  return "half4";
    }
    SkUNREACHABLE;
}

static const char* varying_type_string(Varying::Type type) {
    switch (type) {
        case Varying::Type::kFloat:  return "float";
        case Varying::Type::kFloat2: return "float2";
        case Varying::Type::kFloat3: return "float3";
        case Varying::Type::kFloat4: return "float4";
        case Varying::Type::kHalf:   return "half";
        case Varying::Type::kHalf2:  return "half2";
        case Varying::Type::kHalf3:  return "half3";
        case Varying::Type::kHalf4:  return "half4";
    }
    SkUNREACHABLE;
}

// Non-static for 100% strong-symbol parity with SkMesh.cpp.
std::tuple<bool, SkString>
check_vertex_offsets_and_stride(SkSpan<const Attribute> attributes,
                                size_t                  stride) {
    static_assert(SkMeshSpecification::kMaxStride       <= 2048);
    static_assert(SkMeshSpecification::kMaxAttributes   <= 8);
    static_assert(SkMeshSpecification::kStrideAlignment >= 4);
    static_assert(SkMeshSpecification::kOffsetAlignment >= 4);
    static_assert(SkMeshSpecification::kMaxVaryings     <= 6);
    static_assert(SkIsPow2(SkMeshSpecification::kStrideAlignment));

    STArray<SkMeshSpecification::kMaxAttributes + 1, SkiaRsMeshAttrDesc> descs;
    size_t copyCount = std::min(attributes.size(), SkMeshSpecification::kMaxAttributes + 1);
    for (size_t i = 0; i < copyCount; ++i) {
        descs.push_back(SkiaRsMeshAttrDesc{
                static_cast<uint32_t>(attributes[i].type), attributes[i].offset});
    }

    int32_t err = skia_rs_canvas_mesh_check_layout(descs.data(), attributes.size(), stride);
    switch (err) {
        case 0: RETURN_SUCCESS;
        case 1: RETURN_ERROR("At least 1 attribute is required.");
        case 2: RETURN_ERROR("A maximum of %zu attributes is allowed.",
                             SkMeshSpecification::kMaxAttributes);
        case 3: RETURN_ERROR("Vertex stride must be a non-zero multiple of %zu.",
                             SkMeshSpecification::kStrideAlignment);
        case 4: RETURN_ERROR("Stride cannot exceed %zu.", SkMeshSpecification::kMaxStride);
        case 5: RETURN_ERROR("Attribute offset must be a multiple of %zu.",
                             SkMeshSpecification::kOffsetAlignment);
        case 6: RETURN_ERROR("Attribute offset plus size cannot exceed stride.");
        default: SkUNREACHABLE;
    }
}

// Non-static for 100% strong-symbol parity with SkMesh.cpp.
int check_for_passthrough_local_coords_and_dead_varyings(const SkSL::Program& fsProgram,
                                                         uint32_t* deadVaryingMask) {
    SkASSERT(deadVaryingMask);

    using namespace SkSL;
    static constexpr int kFailed = -2;

    class Visitor final : public SkSL::ProgramVisitor {
    public:
        Visitor(const Context& context) : fContext(context) {}

        void visit(const Program& program) { ProgramVisitor::visit(program); }

        int passthroughFieldIndex() const { return fPassthroughFieldIndex; }
        uint32_t fieldUseMask() const { return fFieldUseMask; }

    protected:
        bool visitProgramElement(const ProgramElement& p) override {
            if (p.is<StructDefinition>()) {
                const auto& def = p.as<StructDefinition>();
                if (def.type().name() == "Varyings") {
                    fVaryingsType = &def.type();
                }
                return false;
            }
            if (p.is<FunctionDefinition>() && p.as<FunctionDefinition>().declaration().isMain()) {
                SkASSERT(!fVaryings);
                fVaryings = p.as<FunctionDefinition>().declaration().parameters()[0];
                SkASSERT(fVaryingsType && fVaryingsType->matches(fVaryings->type()));

                fInMain = true;
                bool result = ProgramVisitor::visitProgramElement(p);
                fInMain = false;
                return result;
            }
            return ProgramVisitor::visitProgramElement(p);
        }

        bool visitStatement(const Statement& s) override {
            if (!fInMain) {
                return ProgramVisitor::visitStatement(s);
            }
            SkASSERT(fVaryings);
            SkASSERT(fVaryingsType);

            if (fPassthroughFieldIndex == kFailed || !s.is<ReturnStatement>()) {
                return ProgramVisitor::visitStatement(s);
            }

            const auto& rs = s.as<ReturnStatement>();
            SkASSERT(rs.expression());
            if (!rs.expression()->is<FieldAccess>()) {
                this->passthroughFailed();
                return ProgramVisitor::visitStatement(s);
            }
            const auto& fa = rs.expression()->as<FieldAccess>();
            if (!fa.base()->is<VariableReference>()) {
                this->passthroughFailed();
                return ProgramVisitor::visitStatement(s);
            }
            const auto& baseRef = fa.base()->as<VariableReference>();
            if (baseRef.variable() != fVaryings) {
                this->passthroughFailed();
                return ProgramVisitor::visitStatement(s);
            }
            if (fPassthroughFieldIndex >= 0) {
                if (fa.fieldIndex() != fPassthroughFieldIndex) {
                    this->passthroughFailed();
                    return ProgramVisitor::visitStatement(s);
                }
                return false;
            }
            const Field& field = fVaryings->type().fields()[fa.fieldIndex()];
            if (!field.fType->matches(*fContext.fTypes.fFloat2)) {
                this->passthroughFailed();
                return ProgramVisitor::visitStatement(s);
            }
            fPassthroughFieldIndex = fa.fieldIndex();
            return false;
        }

        bool visitExpression(const Expression& e) override {
            if (!fVaryingsType) {
                return false;
            }
            if (!e.is<FieldAccess>()) {
                return ProgramVisitor::visitExpression(e);
            }
            const auto& fa = e.as<FieldAccess>();
            if (!fa.base()->type().matches(*fVaryingsType)) {
                return ProgramVisitor::visitExpression(e);
            }
            fFieldUseMask |= 1 << fa.fieldIndex();
            return false;
        }

    private:
        void passthroughFailed() {
            if (fPassthroughFieldIndex >= 0) {
                fFieldUseMask |= 1 << fPassthroughFieldIndex;
            }
            fPassthroughFieldIndex = kFailed;
        }

        const Context& fContext;
        const Type* fVaryingsType = nullptr;
        const Variable* fVaryings = nullptr;
        int fPassthroughFieldIndex = -1;
        bool fInMain = false;
        uint32_t fFieldUseMask = 0;
    };

    Visitor v(*fsProgram.fContext);
    v.visit(fsProgram);
    *deadVaryingMask = ~v.fieldUseMask();
    return v.passthroughFieldIndex();
}

SkMeshSpecification::Result SkMeshSpecification::Make(SkSpan<const Attribute> attributes,
                                                      size_t vertexStride,
                                                      SkSpan<const Varying> varyings,
                                                      const SkString& vs,
                                                      const SkString& fs) {
    return Make(attributes, vertexStride, varyings, vs, fs,
                SkColorSpace::MakeSRGB(), kPremul_SkAlphaType);
}

SkMeshSpecification::Result SkMeshSpecification::Make(SkSpan<const Attribute> attributes,
                                                      size_t vertexStride,
                                                      SkSpan<const Varying> varyings,
                                                      const SkString& vs,
                                                      const SkString& fs,
                                                      sk_sp<SkColorSpace> cs) {
    return Make(attributes, vertexStride, varyings, vs, fs, std::move(cs), kPremul_SkAlphaType);
}

SkMeshSpecification::Result SkMeshSpecification::Make(SkSpan<const Attribute> attributes,
                                                      size_t vertexStride,
                                                      SkSpan<const Varying> varyings,
                                                      const SkString& vs,
                                                      const SkString& fs,
                                                      sk_sp<SkColorSpace> cs,
                                                      SkAlphaType at) {
    SkString attributesStruct("struct Attributes {\n");
    for (const auto& a : attributes) {
        attributesStruct.appendf("  %s %s;\n", attribute_type_string(a.type), a.name.c_str());
    }
    attributesStruct.append("};\n");

    bool userProvidedPositionVarying = false;
    for (const auto& v : varyings) {
        if (v.name.equals("position")) {
            if (v.type != Varying::Type::kFloat2) {
                return {nullptr, SkString("Varying \"position\" must have type float2.")};
            }
            userProvidedPositionVarying = true;
        }
    }

    STArray<kMaxVaryings, Varying> tempVaryings;
    if (!userProvidedPositionVarying) {
        if (varyings.size() > kMaxVaryings - 1) {
            RETURN_FAILURE("A maximum of %zu varyings is allowed.", kMaxVaryings);
        }
        for (const auto& v : varyings) {
            tempVaryings.push_back(v);
        }
        tempVaryings.push_back(Varying{Varying::Type::kFloat2, SkString("position")});
        varyings = tempVaryings;
    }

    SkString varyingStruct("struct Varyings {\n");
    for (const auto& v : varyings) {
        varyingStruct.appendf("  %s %s;\n", varying_type_string(v.type), v.name.c_str());
    }
    varyingStruct.append("};\n");

    SkString fullVS;
    fullVS.append(varyingStruct.c_str());
    fullVS.append(attributesStruct.c_str());
    fullVS.append(vs.c_str());

    SkString fullFS;
    fullFS.append(varyingStruct.c_str());
    fullFS.append(fs.c_str());

    return MakeFromSourceWithStructs(
            attributes, vertexStride, varyings, fullVS, fullFS, std::move(cs), at);
}

SkMeshSpecification::Result SkMeshSpecification::MakeFromSourceWithStructs(
        SkSpan<const Attribute> attributes,
        size_t                  stride,
        SkSpan<const Varying>   varyings,
        const SkString&         vs,
        const SkString&         fs,
        sk_sp<SkColorSpace>     cs,
        SkAlphaType             at) {
    if (auto [ok, error] = check_vertex_offsets_and_stride(attributes, stride); !ok) {
        return {nullptr, error};
    }

    for (const auto& a : attributes) {
        if (!check_name(a.name)) {
            RETURN_FAILURE("\"%s\" is not a valid attribute name.", a.name.c_str());
        }
    }

    if (varyings.size() > kMaxVaryings) {
        RETURN_FAILURE("A maximum of %zu varyings is allowed.", kMaxVaryings);
    }

    for (const auto& v : varyings) {
        if (!check_name(v.name)) {
            return {nullptr, SkStringPrintf("\"%s\" is not a valid varying name.", v.name.c_str())};
        }
    }

    std::vector<Uniform> uniforms;
    std::vector<Child> children;
    size_t offset = 0;

    SkSL::Compiler compiler;
    SkSL::ProgramSettings settings;
    settings.fUseMemoryPool = false;

    std::unique_ptr<SkSL::Program> vsProgram = compiler.convertProgram(
            SkSL::ProgramKind::kMeshVertex, std::string(vs.c_str()), settings);
    if (!vsProgram) {
        RETURN_FAILURE("VS: %s", compiler.errorText().c_str());
    }

    if (auto [result, error] = gather_uniforms_and_check_for_main(
                *vsProgram, &uniforms, &children,
                SkMeshSpecification::Uniform::Flags::kVertex_Flag, &offset);
        !result) {
        return {nullptr, std::move(error)};
    }

    if (SkSL::Analysis::CallsColorTransformIntrinsics(*vsProgram)) {
        RETURN_FAILURE("Color transform intrinsics are not permitted in custom mesh shaders");
    }

    std::unique_ptr<SkSL::Program> fsProgram = compiler.convertProgram(
            SkSL::ProgramKind::kMeshFragment, std::string(fs.c_str()), settings);
    if (!fsProgram) {
        RETURN_FAILURE("FS: %s", compiler.errorText().c_str());
    }

    if (auto [result, error] = gather_uniforms_and_check_for_main(
                *fsProgram, &uniforms, &children,
                SkMeshSpecification::Uniform::Flags::kFragment_Flag, &offset);
        !result) {
        return {nullptr, std::move(error)};
    }

    if (SkSL::Analysis::CallsColorTransformIntrinsics(*fsProgram)) {
        RETURN_FAILURE("Color transform intrinsics are not permitted in custom mesh shaders");
    }

    ColorType ct = get_fs_color_type(*fsProgram);

    if (ct == ColorType::kNone) {
        cs = nullptr;
        at = kPremul_SkAlphaType;
    } else {
        if (!cs) {
            return {nullptr, SkString{"Must provide a color space if FS returns a color."}};
        }
        if (at == kUnknown_SkAlphaType) {
            return {nullptr, SkString{"Must provide a valid alpha type if FS returns a color."}};
        }
    }

    uint32_t deadVaryingMask;
    int passthroughLocalCoordsVaryingIndex =
            check_for_passthrough_local_coords_and_dead_varyings(*fsProgram, &deadVaryingMask);

    if (passthroughLocalCoordsVaryingIndex >= 0) {
        SkASSERT(varyings[passthroughLocalCoordsVaryingIndex].type == Varying::Type::kFloat2);
    }

    return {sk_sp<SkMeshSpecification>(new SkMeshSpecification(attributes,
                                                               stride,
                                                               varyings,
                                                               passthroughLocalCoordsVaryingIndex,
                                                               deadVaryingMask,
                                                               std::move(uniforms),
                                                               std::move(children),
                                                               std::move(vsProgram),
                                                               std::move(fsProgram),
                                                               ct,
                                                               std::move(cs),
                                                               at)),
            /*error=*/{}};
}

SkMeshSpecification::~SkMeshSpecification() = default;

SkMeshSpecification::SkMeshSpecification(
        SkSpan<const Attribute>              attributes,
        size_t                               stride,
        SkSpan<const Varying>                varyings,
        int                                  passthroughLocalCoordsVaryingIndex,
        uint32_t                             deadVaryingMask,
        std::vector<Uniform>                 uniforms,
        std::vector<Child>                   children,
        std::unique_ptr<const SkSL::Program> vs,
        std::unique_ptr<const SkSL::Program> fs,
        ColorType                            ct,
        sk_sp<SkColorSpace>                  cs,
        SkAlphaType                          at)
        : fAttributes(attributes.begin(), attributes.end())
        , fVaryings(varyings.begin(), varyings.end())
        , fUniforms(std::move(uniforms))
        , fChildren(std::move(children))
        , fVS(std::move(vs))
        , fFS(std::move(fs))
        , fStride(stride)
        , fPassthroughLocalCoordsVaryingIndex(passthroughLocalCoordsVaryingIndex)
        , fDeadVaryingMask(deadVaryingMask)
        , fColorType(ct)
        , fColorSpace(std::move(cs))
        , fAlphaType(at) {
    STArray<kMaxAttributes, SkiaRsMeshAttrDesc> descs;
    for (const auto& a : fAttributes) {
        descs.push_back(SkiaRsMeshAttrDesc{static_cast<uint32_t>(a.type), a.offset});
    }
    uint64_t csHash = fColorSpace ? fColorSpace->hash() : 0;
    fHash = skia_rs_canvas_mesh_spec_hash(
            reinterpret_cast<const uint8_t*>(fVS->fSource->c_str()), fVS->fSource->size(),
            reinterpret_cast<const uint8_t*>(fFS->fSource->c_str()), fFS->fSource->size(),
            descs.data(), descs.size(),
            stride, csHash, static_cast<uint32_t>(fAlphaType));
}

size_t SkMeshSpecification::uniformSize() const {
    if (fUniforms.empty()) {
        return skia_rs_canvas_mesh_uniform_size(false, 0, 0);
    }
    return skia_rs_canvas_mesh_uniform_size(
            true, fUniforms.back().offset, fUniforms.back().sizeInBytes());
}

const Uniform* SkMeshSpecification::findUniform(std::string_view name) const {
    for (const Uniform& uniform : fUniforms) {
        if (uniform.name == name) {
            return &uniform;
        }
    }
    return nullptr;
}

const Child* SkMeshSpecification::findChild(std::string_view name) const {
    for (const Child& child : fChildren) {
        if (child.name == name) {
            return &child;
        }
    }
    return nullptr;
}

const Attribute* SkMeshSpecification::findAttribute(std::string_view name) const {
    for (const Attribute& attr : fAttributes) {
        if (name == attr.name.c_str()) {
            return &attr;
        }
    }
    return nullptr;
}

const Varying* SkMeshSpecification::findVarying(std::string_view name) const {
    for (const Varying& varying : fVaryings) {
        if (name == varying.name.c_str()) {
            return &varying;
        }
    }
    return nullptr;
}

SkMesh::SkMesh()  = default;
SkMesh::~SkMesh() = default;
SkMesh::SkMesh(const SkMesh&) = default;
SkMesh::SkMesh(SkMesh&&)      = default;
SkMesh& SkMesh::operator=(const SkMesh&) = default;
SkMesh& SkMesh::operator=(SkMesh&&)      = default;

SkMesh::Result SkMesh::Make(sk_sp<SkMeshSpecification> spec,
                            Mode mode,
                            sk_sp<VertexBuffer> vb,
                            size_t vertexCount,
                            size_t vertexOffset,
                            sk_sp<const SkData> uniforms,
                            SkSpan<ChildPtr> children,
                            const SkRect& bounds) {
    SkMesh mesh;
    mesh.fSpec     = std::move(spec);
    mesh.fMode     = mode;
    mesh.fVB       = std::move(vb);
    mesh.fUniforms = std::move(uniforms);
    mesh.fChildren.push_back_n(children.size(), children.data());
    mesh.fVCount   = vertexCount;
    mesh.fVOffset  = vertexOffset;
    mesh.fBounds   = bounds;
    auto [valid, msg] = mesh.validate();
    if (!valid) {
        mesh = {};
    }
    return {std::move(mesh), std::move(msg)};
}

SkMesh::Result SkMesh::MakeIndexed(sk_sp<SkMeshSpecification> spec,
                                   Mode mode,
                                   sk_sp<VertexBuffer> vb,
                                   size_t vertexCount,
                                   size_t vertexOffset,
                                   sk_sp<IndexBuffer> ib,
                                   size_t indexCount,
                                   size_t indexOffset,
                                   sk_sp<const SkData> uniforms,
                                   SkSpan<ChildPtr> children,
                                   const SkRect& bounds) {
    if (!ib) {
        return {{}, SkString{"An index buffer is required."}};
    }
    SkMesh mesh;
    mesh.fSpec     = std::move(spec);
    mesh.fMode     = mode;
    mesh.fVB       = std::move(vb);
    mesh.fVCount   = vertexCount;
    mesh.fVOffset  = vertexOffset;
    mesh.fIB       = std::move(ib);
    mesh.fUniforms = std::move(uniforms);
    mesh.fChildren.push_back_n(children.size(), children.data());
    mesh.fICount   = indexCount;
    mesh.fIOffset  = indexOffset;
    mesh.fBounds   = bounds;
    auto [valid, msg] = mesh.validate();
    if (!valid) {
        mesh = {};
    }
    return {std::move(mesh), std::move(msg)};
}

bool SkMesh::isValid() const {
    bool valid = SkToBool(fSpec);
    SkASSERT(valid == std::get<0>(this->validate()));
    return valid;
}

std::tuple<bool, SkString> SkMesh::validate() const {
#define FAIL_MESH_VALIDATE(...) return std::make_tuple(false, SkStringPrintf(__VA_ARGS__))
    if (!fSpec) {
        FAIL_MESH_VALIDATE("SkMeshSpecification is required.");
    }
    if (!fVB) {
        FAIL_MESH_VALIDATE("A vertex buffer is required.");
    }
    if (fSpec->children().size() != SkToSizeT(fChildren.size())) {
        FAIL_MESH_VALIDATE("The mesh specification declares %zu child effects, "
                           "but the mesh supplies %d.",
                           fSpec->children().size(),
                           fChildren.size());
    }

    for (int index = 0; index < fChildren.size(); ++index) {
        const SkRuntimeEffect::Child& meshSpecChild = fSpec->children()[index];
        if (fChildren[index].type().has_value()) {
            if (meshSpecChild.type != fChildren[index].type()) {
                FAIL_MESH_VALIDATE("Child effect '%.*s' was specified as a %s, but passed as a %s.",
                                   (int)meshSpecChild.name.size(), meshSpecChild.name.data(),
                                   SkRuntimeEffectPriv::ChildTypeToStr(meshSpecChild.type),
                                   SkRuntimeEffectPriv::ChildTypeToStr(*fChildren[index].type()));
            }
        }
    }

    auto vb = static_cast<SkMeshPriv::VB*>(fVB.get());
    auto ib = static_cast<SkMeshPriv::IB*>(fIB.get());
    const size_t uniformSize = fSpec->uniformSize();

    int32_t err = skia_rs_canvas_mesh_validate_buffers(
            fSpec->stride(), fVCount, fVOffset, vb->size(),
            uniformSize, fUniforms != nullptr, fUniforms ? fUniforms->size() : 0,
            ib != nullptr, ib ? ib->size() : 0, fICount, fIOffset,
            static_cast<int32_t>(fMode));

    auto modeToStr = [](Mode m) {
        switch (m) {
            case Mode::kTriangles:     return "triangles";
            case Mode::kTriangleStrip: return "triangle-strip";
        }
        SkUNREACHABLE;
    };

    switch (err) {
        case 0:
            if (!ib) {
                SkASSERT(!fICount);
                SkASSERT(!fIOffset);
            }
            return {true, {}};
        case 1:
            FAIL_MESH_VALIDATE("The vertex buffer offset and vertex count reads beyond the end of the"
                               " vertex buffer.");
        case 2:
            FAIL_MESH_VALIDATE("The vertex offset (%zu) must be a multiple of the vertex stride (%zu).",
                               fVOffset, fSpec->stride());
        case 3:
            FAIL_MESH_VALIDATE("The uniform data is %zu bytes but must be at least %zu.",
                               fUniforms ? fUniforms->size() : 0, uniformSize);
        case 4:
            FAIL_MESH_VALIDATE("%s mode requires at least %zu indices but index count is %zu.",
                               modeToStr(fMode), size_t{3}, fICount);
        case 5:
            FAIL_MESH_VALIDATE("The index buffer offset and index count reads beyond the end of the"
                               " index buffer.");
        case 6:
            FAIL_MESH_VALIDATE("The index offset must be a multiple of 2.");
        case 7:
            FAIL_MESH_VALIDATE("%s mode requires at least %zu vertices but vertex count is %zu.",
                               modeToStr(fMode), size_t{3}, fICount);
        case 8:
            FAIL_MESH_VALIDATE("Overflow");
        default:
            SkUNREACHABLE;
    }
#undef FAIL_MESH_VALIDATE
}

bool SkMesh::IndexBuffer::update(GrDirectContext* dc,
                                 const void* data,
                                 size_t offset,
                                 size_t size) {
    return skia_rs_canvas_mesh_check_update(data != nullptr, offset, size, this->size()) &&
           this->onUpdate(dc, data, offset, size);
}

bool SkMesh::VertexBuffer::update(GrDirectContext* dc,
                                  const void* data,
                                  size_t offset,
                                  size_t size) {
    return skia_rs_canvas_mesh_check_update(data != nullptr, offset, size, this->size()) &&
           this->onUpdate(dc, data, offset, size);
}

namespace SkMeshes {
sk_sp<IndexBuffer> MakeIndexBuffer(const void* data, size_t size) {
    return SkMeshPriv::CpuIndexBuffer::Make(data, size);
}

sk_sp<IndexBuffer> CopyIndexBuffer(const sk_sp<IndexBuffer>& src) {
    if (!src) {
        return nullptr;
    }
    auto* ib = static_cast<SkMeshPriv::IB*>(src.get());
    const void* data = ib->peek();
    if (!data) {
        return nullptr;
    }
    return MakeIndexBuffer(data, ib->size());
}

sk_sp<VertexBuffer> MakeVertexBuffer(const void* data, size_t size) {
    return SkMeshPriv::CpuVertexBuffer::Make(data, size);
}

sk_sp<VertexBuffer> CopyVertexBuffer(const sk_sp<VertexBuffer>& src) {
    if (!src) {
        return nullptr;
    }
    auto* vb = static_cast<SkMeshPriv::VB*>(src.get());
    const void* data = vb->peek();
    if (!data) {
        return nullptr;
    }
    return MakeVertexBuffer(data, vb->size());
}
}  // namespace SkMeshes

#undef RETURN_FAILURE
#undef RETURN_ERROR
#undef RETURN_SUCCESS
