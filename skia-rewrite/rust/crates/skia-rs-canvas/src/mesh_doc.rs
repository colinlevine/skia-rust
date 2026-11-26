//! Safe Rust mesh specification, mesh validation, and document state machine matching:
//! - `src/core/SkMesh.cpp`
//! - `src/core/SkDocument.cpp`
//! - `src/core/SkCapabilities.cpp`
//! - `src/core/SkGraphics.cpp`
//! - `src/core/SkGlobalInitialization_core.cpp`

use skia_rs_core_types::{align4, is_align4};
use skia_rs_geometry::Rect;
use skia_rs_math::checksum::hash32;

/// Constants matching `SkMeshSpecification`.
pub const MESH_MAX_STRIDE: usize = 2048;
pub const MESH_MAX_ATTRIBUTES: usize = 8;
pub const MESH_STRIDE_ALIGNMENT: usize = 4;
pub const MESH_OFFSET_ALIGNMENT: usize = 4;
pub const MESH_MAX_VARYINGS: usize = 6;

/// `SkMeshSpecification::Attribute::Type` values.
pub const ATTR_TYPE_FLOAT: u32 = 0;
pub const ATTR_TYPE_FLOAT2: u32 = 1;
pub const ATTR_TYPE_FLOAT3: u32 = 2;
pub const ATTR_TYPE_FLOAT4: u32 = 3;
pub const ATTR_TYPE_UBYTE4_UNORM: u32 = 4;

/// `SkMeshSpecification::Varying::Type` values.
pub const VARYING_TYPE_FLOAT: u32 = 0;
pub const VARYING_TYPE_FLOAT2: u32 = 1;
pub const VARYING_TYPE_FLOAT3: u32 = 2;
pub const VARYING_TYPE_FLOAT4: u32 = 3;
pub const VARYING_TYPE_HALF: u32 = 4;
pub const VARYING_TYPE_HALF2: u32 = 5;
pub const VARYING_TYPE_HALF3: u32 = 6;
pub const VARYING_TYPE_HALF4: u32 = 7;

/// `SkMesh::Mode` values.
pub const MESH_MODE_TRIANGLES: i32 = 0;
pub const MESH_MODE_TRIANGLE_STRIP: i32 = 1;

/// `SkDocument::State` values.
pub const DOC_STATE_BETWEEN_PAGES: i32 = 0;
pub const DOC_STATE_IN_PAGE: i32 = 1;
pub const DOC_STATE_CLOSED: i32 = 2;

/// Compact attribute layout descriptor for FFI validation and hashing.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MeshAttrDesc {
    pub attr_type: u32,
    pub offset: usize,
}

/// Error code for `check_vertex_offsets_and_stride`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MeshLayoutError {
    Ok = 0,
    NoAttributes = 1,
    TooManyAttributes = 2,
    BadStrideAlignment = 3,
    StrideTooLarge = 4,
    BadOffsetAlignment = 5,
    OffsetExceedsStride = 6,
}

/// Error code for `SkMesh::validate` buffer & count checks.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MeshBufferError {
    Ok = 0,
    VertexOutOfBounds = 1,
    VertexOffsetMisaligned = 2,
    UniformDataTooSmall = 3,
    TooFewIndices = 4,
    IndexOutOfBounds = 5,
    IndexOffsetMisaligned = 6,
    TooFewVertices = 7,
    Overflow = 8,
}

/// Result of `SkDocument::beginPage` content trim.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DocumentTrimResult {
    pub valid: bool,
    pub inner: Rect,
}

/// Matches `check_name` in `SkMesh.cpp` (`'_'` or ASCII alphanumeric in classic locale).
#[inline]
pub fn check_mesh_var_name(name: &[u8]) -> bool {
    if name.is_empty() {
        return false;
    }
    name.iter().all(|&b| b == b'_' || b.is_ascii_alphanumeric())
}

/// Matches `attribute_type_size` in `SkMesh.cpp`.
#[inline]
pub fn attribute_type_size(attr_type: u32) -> usize {
    match attr_type {
        ATTR_TYPE_FLOAT => 4,
        ATTR_TYPE_FLOAT2 => 8,
        ATTR_TYPE_FLOAT3 => 12,
        ATTR_TYPE_FLOAT4 => 16,
        ATTR_TYPE_UBYTE4_UNORM => 4,
        _ => 0,
    }
}

/// Matches `min_vcount_for_mode` in `SkMesh.cpp`.
#[inline]
pub fn min_vcount_for_mode(mode: i32) -> usize {
    match mode {
        MESH_MODE_TRIANGLES | MESH_MODE_TRIANGLE_STRIP => 3,
        _ => 3,
    }
}

/// Matches `check_vertex_offsets_and_stride` in `SkMesh.cpp`.
pub fn check_vertex_offsets_and_stride(
    attrs: &[MeshAttrDesc],
    stride: usize,
) -> MeshLayoutError {
    if attrs.is_empty() {
        return MeshLayoutError::NoAttributes;
    }
    if attrs.len() > MESH_MAX_ATTRIBUTES {
        return MeshLayoutError::TooManyAttributes;
    }
    if stride == 0 || (stride & (MESH_STRIDE_ALIGNMENT - 1)) != 0 {
        return MeshLayoutError::BadStrideAlignment;
    }
    if stride > MESH_MAX_STRIDE {
        return MeshLayoutError::StrideTooLarge;
    }
    for a in attrs {
        if (a.offset & (MESH_OFFSET_ALIGNMENT - 1)) != 0 {
            return MeshLayoutError::BadOffsetAlignment;
        }
        let sz = attribute_type_size(a.attr_type);
        if a.offset >= stride || a.offset + sz > stride {
            return MeshLayoutError::OffsetExceedsStride;
        }
    }
    MeshLayoutError::Ok
}

/// Matches `SkMeshSpecification::uniformSize`.
#[inline]
pub fn mesh_spec_uniform_size(has_uniforms: bool, last_offset: usize, last_size: usize) -> usize {
    if !has_uniforms {
        0
    } else {
        align4(last_offset + last_size)
    }
}

/// Matches `SkMeshSpecification` constructor hash calculation (`fHash`).
pub fn mesh_spec_compute_hash(
    vs_src: &[u8],
    fs_src: &[u8],
    attrs: &[MeshAttrDesc],
    stride: usize,
    cs_hash: u64,
    alpha_type: u32,
) -> u32 {
    let mut h = hash32(vs_src, 0);
    h = hash32(fs_src, h);
    for a in attrs {
        h = hash32(&a.offset.to_ne_bytes(), h);
        h = hash32(&a.attr_type.to_ne_bytes(), h);
    }
    h = hash32(&stride.to_ne_bytes(), h);
    h = hash32(&cs_hash.to_ne_bytes(), h);
    h = hash32(&alpha_type.to_ne_bytes(), h);
    h
}

/// Matches buffer/count validation in `SkMesh::validate`.
#[allow(clippy::too_many_arguments)]
pub fn validate_mesh_buffers(
    stride: usize,
    vcount: usize,
    voffset: usize,
    vb_size: usize,
    spec_uniform_size: usize,
    has_uniforms: bool,
    uniforms_size: usize,
    has_ib: bool,
    ib_size: usize,
    icount: usize,
    ioffset: usize,
    mode: i32,
) -> MeshBufferError {
    let mut ok = true;
    let vsize = match stride.checked_mul(vcount) {
        Some(v) => v,
        None => {
            ok = false;
            usize::MAX
        }
    };
    let vend = match vsize.checked_add(voffset) {
        Some(v) => v,
        None => {
            ok = false;
            usize::MAX
        }
    };
    if vend > vb_size {
        return MeshBufferError::VertexOutOfBounds;
    }
    if stride == 0 || (voffset % stride) != 0 {
        return MeshBufferError::VertexOffsetMisaligned;
    }
    if spec_uniform_size != 0 && (!has_uniforms || uniforms_size < spec_uniform_size) {
        return MeshBufferError::UniformDataTooSmall;
    }

    let min_cnt = min_vcount_for_mode(mode);
    if has_ib {
        if icount < min_cnt {
            return MeshBufferError::TooFewIndices;
        }
        let isize = match 2usize.checked_mul(icount) {
            Some(v) => v,
            None => {
                ok = false;
                usize::MAX
            }
        };
        let iend = match isize.checked_add(ioffset) {
            Some(v) => v,
            None => {
                ok = false;
                usize::MAX
            }
        };
        if iend > ib_size {
            return MeshBufferError::IndexOutOfBounds;
        }
        if (ioffset & 1) != 0 {
            return MeshBufferError::IndexOffsetMisaligned;
        }
    } else if vcount < min_cnt {
        return MeshBufferError::TooFewVertices;
    }

    if !ok {
        return MeshBufferError::Overflow;
    }
    MeshBufferError::Ok
}

/// Matches `check_update` in `SkMesh.cpp`.
#[inline]
pub fn check_mesh_buffer_update(
    has_data: bool,
    offset: usize,
    size: usize,
    buffer_size: usize,
) -> bool {
    if !has_data || size == 0 || !is_align4(offset) || !is_align4(size) {
        return false;
    }
    match offset.checked_add(size) {
        Some(end) => end <= buffer_size,
        None => false,
    }
}

/// Matches `trim` in `SkDocument.cpp`.
pub fn document_trim_content(width: f32, height: f32, content: Rect) -> DocumentTrimResult {
    let mut inner = content;
    let page = Rect::from_ltrb(0.0, 0.0, width, height);
    if inner.intersect(page) {
        DocumentTrimResult { valid: true, inner }
    } else {
        DocumentTrimResult {
            valid: false,
            inner: Rect::default(),
        }
    }
}

/// Matches `SkDocument::beginPage` eligibility check (`width <= 0 || height <= 0 || closed`).
#[inline]
pub fn document_can_begin_page(width: f32, height: f32, state: i32) -> bool {
    !(width <= 0.0 || height <= 0.0 || state == DOC_STATE_CLOSED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mesh_var_name() {
        assert!(check_mesh_var_name(b"position"));
        assert!(check_mesh_var_name(b"_uv_0"));
        assert!(!check_mesh_var_name(b""));
        assert!(!check_mesh_var_name(b"foo bar"));
        assert!(!check_mesh_var_name(b"a;b"));
    }

    #[test]
    fn test_mesh_layout_checks() {
        assert_eq!(check_vertex_offsets_and_stride(&[], 8), MeshLayoutError::NoAttributes);
        let a = [MeshAttrDesc {
            attr_type: ATTR_TYPE_FLOAT2,
            offset: 0,
        }];
        assert_eq!(check_vertex_offsets_and_stride(&a, 8), MeshLayoutError::Ok);
        assert_eq!(
            check_vertex_offsets_and_stride(&a, 6),
            MeshLayoutError::BadStrideAlignment
        );
        assert_eq!(
            check_vertex_offsets_and_stride(&a, 4),
            MeshLayoutError::OffsetExceedsStride
        );
        assert_eq!(
            check_vertex_offsets_and_stride(
                &[MeshAttrDesc {
                    attr_type: ATTR_TYPE_FLOAT,
                    offset: 2
                }],
                8
            ),
            MeshLayoutError::BadOffsetAlignment
        );
    }

    #[test]
    fn test_mesh_buffer_validation() {
        assert_eq!(
            validate_mesh_buffers(8, 3, 0, 24, 0, false, 0, false, 0, 0, 0, MESH_MODE_TRIANGLES),
            MeshBufferError::Ok
        );
        assert_eq!(
            validate_mesh_buffers(8, 2, 0, 24, 0, false, 0, false, 0, 0, 0, MESH_MODE_TRIANGLES),
            MeshBufferError::TooFewVertices
        );
        assert_eq!(
            validate_mesh_buffers(8, 3, 4, 32, 0, false, 0, false, 0, 0, 0, MESH_MODE_TRIANGLES),
            MeshBufferError::VertexOffsetMisaligned
        );
        assert_eq!(
            validate_mesh_buffers(8, 3, 0, 24, 0, false, 0, true, 6, 3, 1, MESH_MODE_TRIANGLES),
            MeshBufferError::IndexOutOfBounds
        );
        assert_eq!(
            validate_mesh_buffers(8, 3, 0, 24, 0, false, 0, true, 8, 3, 1, MESH_MODE_TRIANGLES),
            MeshBufferError::IndexOffsetMisaligned
        );
    }

    #[test]
    fn test_mesh_update_and_doc_trim() {
        assert!(check_mesh_buffer_update(true, 4, 8, 12));
        assert!(!check_mesh_buffer_update(true, 2, 8, 12));
        assert!(!check_mesh_buffer_update(true, 4, 12, 12));
        assert!(!check_mesh_buffer_update(false, 0, 4, 12));

        let t = document_trim_content(100.0, 200.0, Rect::from_ltrb(-10.0, 50.0, 80.0, 250.0));
        assert!(t.valid);
        assert_eq!(t.inner, Rect::from_ltrb(0.0, 50.0, 80.0, 200.0));
    }
}
