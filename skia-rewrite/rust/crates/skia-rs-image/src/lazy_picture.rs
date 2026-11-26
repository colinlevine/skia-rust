//! Safe Rust port of `src/image/SkImage_Lazy.cpp`, `src/image/SkImage_LazyFactories.cpp`,
//! and `src/image/SkImage_Picture.cpp` algorithms and planning logic.

use skia_rs_bitmap::pixel_ref::next_image_id;
use skia_rs_color::image_info::ColorType;
use skia_rs_geometry::{
    matrix::{MTRANS_X, MTRANS_Y, TRANSLATE_MASK},
    IRect, ISize, Matrix,
};

/// Matches `SkTiledImageUtils::kNumImageKeyValues`.
pub const NUM_IMAGE_KEY_VALUES: usize = 6;

/// Matches `SkImages::BitDepth`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BitDepth {
    U8 = 0,
    F16 = 1,
}

/// Result of `SkImage_Lazy::Validator::Validator`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LazyValidatorPlan {
    pub valid: bool,
    pub apply_color_type: bool,
    pub color_type: ColorType,
    pub apply_color_space: bool,
    pub unique_id: u32,
}

/// Exact port of `SkImage_Lazy::Validator::Validator` logic.
pub fn plan_lazy_validator(
    has_generator: bool,
    gen_width: i32,
    gen_height: i32,
    gen_ct: ColorType,
    gen_unique_id: u32,
    has_color_type: bool,
    requested_ct: ColorType,
    has_color_space: bool,
) -> LazyValidatorPlan {
    if !has_generator || gen_width <= 0 || gen_height <= 0 {
        return LazyValidatorPlan {
            valid: false,
            apply_color_type: false,
            color_type: ColorType::Unknown,
            apply_color_space: false,
            unique_id: 0,
        };
    }

    let apply_ct = has_color_type && requested_ct != gen_ct;
    let unique_id = if apply_ct || has_color_space {
        next_image_id()
    } else {
        gen_unique_id
    };

    LazyValidatorPlan {
        valid: true,
        apply_color_type: apply_ct,
        color_type: if apply_ct { requested_ct } else { gen_ct },
        apply_color_space: has_color_space,
        unique_id,
    }
}

/// Exact port of `SkImage_Lazy::onRefEncoded` eligibility check:
/// only returns encoded data if this image has not been modified (subset / colorType / colorSpace).
#[inline]
pub const fn lazy_can_ref_encoded(gen_unique_id: u32, img_unique_id: u32) -> bool {
    gen_unique_id == img_unique_id
}

/// Exact port of `SkImage_Lazy::makeColorTypeAndColorSpace` cached result hit test.
#[inline]
pub const fn lazy_color_type_and_space_cache_hit(
    has_cached: bool,
    target_ct: ColorType,
    cached_ct: ColorType,
    same_color_space: bool,
) -> bool {
    has_cached && (target_ct as i32) == (cached_ct as i32) && same_color_space
}

/// Exact port of `SkImage_Lazy::getPlanes` YUVA dimension check.
#[inline]
pub const fn lazy_yuva_dimensions_match(
    yuva_w: i32,
    yuva_h: i32,
    img_w: i32,
    img_h: i32,
) -> bool {
    yuva_w == img_w && yuva_h == img_h
}

/// Result of `SkImage_Picture::onMakeSubset` geometry and bit-depth computation.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PictureSubsetPlan {
    pub matrix: Matrix,
    pub dimensions: ISize,
    pub bit_depth: BitDepth,
}

/// Exact port of `SkImage_Picture::onMakeSubset` matrix translation and bit-depth selection.
pub fn plan_picture_subset(
    orig_matrix: Matrix,
    subset: IRect,
    color_type: ColorType,
) -> PictureSubsetPlan {
    let mut matrix = orig_matrix;
    matrix.post_translate((-subset.left) as f32, (-subset.top) as f32);
    let bit_depth = if color_type == ColorType::RgbaF16 {
        BitDepth::F16
    } else {
        BitDepth::U8
    };
    PictureSubsetPlan {
        matrix,
        dimensions: ISize {
            width: subset.width(),
            height: subset.height(),
        },
        bit_depth,
    }
}

/// Exact port of `SkImage_Picture::getImageKeyValues`.
pub fn compute_picture_image_key_values(
    has_paint: bool,
    is_srgb: bool,
    matrix: &Matrix,
    color_type: ColorType,
    pixel_geometry: u32,
    surface_prop_flags: u32,
    width: i32,
    height: i32,
    picture_unique_id: u32,
) -> Option<[u32; NUM_IMAGE_KEY_VALUES]> {
    if has_paint {
        return None;
    }
    if !is_srgb {
        return None;
    }
    let mtype = matrix.get_type_cached();
    // Matches `!m.isIdentity() && !m.isTranslate()`
    if (mtype & !TRANSLATE_MASK) != 0 {
        return None;
    }

    let is_u8 = color_type != ColorType::RgbaF16;
    let key0 = (if is_u8 { 0x1u32 } else { 0x0u32 })
        | (pixel_geometry << 1)
        | (surface_prop_flags << 4);
    let trans_x = matrix.mat[MTRANS_X];
    let trans_y = matrix.mat[MTRANS_Y];

    Some([
        key0,
        picture_unique_id,
        width as u32,
        height as u32,
        trans_x.to_bits(),
        trans_y.to_bits(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lazy_validator_empty_or_null_fails() {
        let p = plan_lazy_validator(
            false,
            10,
            10,
            ColorType::Rgba8888,
            100,
            false,
            ColorType::Unknown,
            false,
        );
        assert!(!p.valid);

        let p2 = plan_lazy_validator(
            true,
            0,
            10,
            ColorType::Rgba8888,
            100,
            false,
            ColorType::Unknown,
            false,
        );
        assert!(!p2.valid);
    }

    #[test]
    fn lazy_validator_preserves_id_when_unchanged() {
        let p = plan_lazy_validator(
            true,
            32,
            32,
            ColorType::Rgba8888,
            777,
            true,
            ColorType::Rgba8888, // same as gen_ct -> ignored!
            false,
        );
        assert!(p.valid);
        assert!(!p.apply_color_type);
        assert!(!p.apply_color_space);
        assert_eq!(p.unique_id, 777);
    }

    #[test]
    fn lazy_validator_allocates_new_id_when_ct_or_cs_changes() {
        let p = plan_lazy_validator(
            true,
            32,
            32,
            ColorType::Rgba8888,
            777,
            true,
            ColorType::Bgra8888,
            false,
        );
        assert!(p.valid);
        assert!(p.apply_color_type);
        assert_eq!(p.color_type, ColorType::Bgra8888);
        assert_ne!(p.unique_id, 777);
        assert_ne!(p.unique_id, 0);

        let p_cs = plan_lazy_validator(
            true,
            32,
            32,
            ColorType::Rgba8888,
            777,
            false,
            ColorType::Unknown,
            true,
        );
        assert!(p_cs.valid);
        assert!(!p_cs.apply_color_type);
        assert!(p_cs.apply_color_space);
        assert_ne!(p_cs.unique_id, 777);
    }

    #[test]
    fn picture_subset_translates_matrix_and_picks_bit_depth() {
        let m = Matrix::translate(10.0, 20.0);
        let plan = plan_picture_subset(m, IRect::from_ltrb(3, 5, 13, 25), ColorType::RgbaF16);
        assert_eq!(plan.dimensions, ISize { width: 10, height: 20 });
        assert_eq!(plan.bit_depth, BitDepth::F16);
        assert_eq!(plan.matrix.mat[MTRANS_X], 7.0);
        assert_eq!(plan.matrix.mat[MTRANS_Y], 15.0);

        let plan_u8 = plan_picture_subset(
            Matrix::identity(),
            IRect::from_ltrb(1, 2, 5, 6),
            ColorType::Rgba8888,
        );
        assert_eq!(plan_u8.bit_depth, BitDepth::U8);
        assert_eq!(plan_u8.matrix.mat[MTRANS_X], -1.0);
        assert_eq!(plan_u8.matrix.mat[MTRANS_Y], -2.0);
    }

    #[test]
    fn picture_image_key_values_packing() {
        let m = Matrix::translate(-4.5, 12.25);
        let keys = compute_picture_image_key_values(
            false,
            true,
            &m,
            ColorType::Rgba8888,
            2, // kBGR_H_SkPixelGeometry
            5, // flags
            100,
            200,
            999,
        )
        .unwrap();
        assert_eq!(keys[0], 1 | (2 << 1) | (5 << 4));
        assert_eq!(keys[1], 999);
        assert_eq!(keys[2], 100);
        assert_eq!(keys[3], 200);
        assert_eq!(f32::from_bits(keys[4]), -4.5);
        assert_eq!(f32::from_bits(keys[5]), 12.25);

        // Paint present -> None
        assert!(compute_picture_image_key_values(
            true,
            true,
            &m,
            ColorType::Rgba8888,
            0,
            0,
            100,
            200,
            999
        )
        .is_none());

        // Non-sRGB -> None
        assert!(compute_picture_image_key_values(
            false,
            false,
            &m,
            ColorType::Rgba8888,
            0,
            0,
            100,
            200,
            999
        )
        .is_none());

        // Scale matrix -> None
        let mut scaled = Matrix::identity();
        scaled.set_scale(2.0, 2.0);
        assert!(compute_picture_image_key_values(
            false,
            true,
            &scaled,
            ColorType::Rgba8888,
            0,
            0,
            100,
            200,
            999
        )
        .is_none());
    }
}
