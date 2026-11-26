//! Safe Rust implementation of `SkRecordOpts` (`src/core/SkRecordOpts.cpp` and `SkRecordPattern.h`).

use crate::record::{is_draw_type, is_single_draw_type, RecordType};

/// Matches `SkMulDiv255Round(U16CPU a, U16CPU b)` = `SkMul16ShiftRound(a, b, 8)`:
/// `let prod = a * b + 128; ((prod + (prod >> 8)) >> 8) as u8`.
#[inline]
pub const fn mul_div_255_round(a: u8, b: u8) -> u8 {
    let prod = (a as u32) * (b as u32) + 128;
    ((prod + (prod >> 8)) >> 8) as u8
}

/// Summary of `SkPaint` properties needed by `SkRecordOpts` optimization passes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PaintOptInfo {
    pub present: bool,
    pub is_src_over: bool,
    pub is_src_mode: bool,
    pub has_shader: bool,
    pub has_path_effect: bool,
    pub has_mask_filter: bool,
    pub has_color_filter: bool,
    pub has_image_filter: bool,
    pub alpha: u8,
    /// Full 32-bit `SkColor` (`paint->getColor()`).
    pub color: u32,
}

/// Decision returned by `eval_save_layer_draw_restore` and `eval_svg_opacity_filter_merge`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayerFoldDecision {
    pub kill_save_layer_and_restore: bool,
    pub update_paint_alpha: bool,
    pub new_alpha: u8,
}

/// Matches `effectively_srcover(const SkPaint* paint)` in `SkRecordOpts.cpp`.
#[inline]
pub const fn effectively_srcover(paint: &PaintOptInfo) -> bool {
    if !paint.present || paint.is_src_over {
        return true;
    }
    !paint.has_shader
        && !paint.has_color_filter
        && !paint.has_image_filter
        && paint.alpha == 0xFF
        && paint.is_src_mode
}

/// Matches `fold_opacity_layer_color_to_paint(const SkPaint* layerPaint, bool isSaveLayer, SkPaint* paint)`.
/// Returns `Some(Some(new_alpha))` if `paint->setAlpha(new_alpha)` should be called,
/// `Some(None)` if folding succeeds with no paint alpha change (`layerPaint == nullptr`),
/// or `None` if folding is disallowed.
pub const fn fold_opacity_layer_color_to_paint(
    layer_paint: &PaintOptInfo,
    is_save_layer: bool,
    paint: &PaintOptInfo,
) -> Option<Option<u8>> {
    if !paint.is_src_over {
        return None;
    }
    if !is_save_layer && paint.has_image_filter {
        return None;
    }
    if paint.has_color_filter {
        return None;
    }
    if layer_paint.present {
        // SK_ColorTRANSPARENT != SkColorSetA(layerColor, SK_AlphaTRANSPARENT)
        // i.e. RGB channels must all be 0.
        if (layer_paint.color & 0x00FF_FFFF) != 0 {
            return None;
        }
        if layer_paint.has_path_effect
            || layer_paint.has_shader
            || !layer_paint.is_src_over
            || layer_paint.has_mask_filter
            || layer_paint.has_color_filter
            || layer_paint.has_image_filter
        {
            return None;
        }
        let layer_alpha = (layer_paint.color >> 24) as u8;
        let new_alpha = mul_div_255_round(paint.alpha, layer_alpha);
        return Some(Some(new_alpha));
    }
    Some(None)
}

/// Evaluates `SaveLayerDrawRestoreNooper::onMatch`.
pub const fn eval_save_layer_draw_restore(
    has_backdrop: bool,
    has_filters: bool,
    layer_paint: &PaintOptInfo,
    draw_paint: &PaintOptInfo,
) -> LayerFoldDecision {
    let none = LayerFoldDecision {
        kill_save_layer_and_restore: false,
        update_paint_alpha: false,
        new_alpha: 0,
    };
    if has_backdrop || has_filters {
        return none;
    }
    if !layer_paint.present && effectively_srcover(draw_paint) {
        return LayerFoldDecision {
            kill_save_layer_and_restore: true,
            update_paint_alpha: false,
            new_alpha: 0,
        };
    }
    if !draw_paint.present {
        return none;
    }
    match fold_opacity_layer_color_to_paint(layer_paint, false, draw_paint) {
        Some(Some(new_alpha)) => LayerFoldDecision {
            kill_save_layer_and_restore: true,
            update_paint_alpha: true,
            new_alpha,
        },
        Some(None) => LayerFoldDecision {
            kill_save_layer_and_restore: true,
            update_paint_alpha: false,
            new_alpha: 0,
        },
        None => none,
    }
}

/// Evaluates `SvgOpacityAndFilterLayerMergePass::onMatch`.
pub const fn eval_svg_opacity_filter_merge(
    outer_has_backdrop: bool,
    outer_has_filters: bool,
    inner_has_filters: bool,
    opacity_paint: &PaintOptInfo,
    filter_layer_paint: &PaintOptInfo,
) -> LayerFoldDecision {
    let none = LayerFoldDecision {
        kill_save_layer_and_restore: false,
        update_paint_alpha: false,
        new_alpha: 0,
    };
    if outer_has_backdrop || outer_has_filters || inner_has_filters {
        return none;
    }
    if !opacity_paint.present {
        return LayerFoldDecision {
            kill_save_layer_and_restore: true,
            update_paint_alpha: false,
            new_alpha: 0,
        };
    }
    if !filter_layer_paint.present {
        return none;
    }
    match fold_opacity_layer_color_to_paint(opacity_paint, true, filter_layer_paint) {
        Some(Some(new_alpha)) => LayerFoldDecision {
            kill_save_layer_and_restore: true,
            update_paint_alpha: true,
            new_alpha,
        },
        Some(None) => LayerFoldDecision {
            kill_save_layer_and_restore: true,
            update_paint_alpha: false,
            new_alpha: 0,
        },
        None => none,
    }
}

/// Exact port of `SaveOnlyDrawsRestoreNooper` single pass over `op_types`.
fn pass_save_only_draws_restore(op_types: &mut [u32], noop_mask: &mut [bool]) -> bool {
    let count = op_types.len();
    let mut changed = false;
    let mut end = 0usize;

    let mut begin = end;
    while begin < count {
        let matched_end = if op_types[begin] == RecordType::Save as u32 {
            let mut i = begin + 1;
            while i < count && (op_types[i] == RecordType::NoOp as u32 || is_draw_type(op_types[i]))
            {
                i += 1;
            }
            if i < count && op_types[i] == RecordType::Restore as u32 {
                i + 1
            } else {
                0
            }
        } else {
            0
        };

        if matched_end != 0 {
            end = matched_end;
            op_types[begin] = RecordType::NoOp as u32;
            op_types[end - 1] = RecordType::NoOp as u32;
            noop_mask[begin] = true;
            noop_mask[end - 1] = true;
            changed = true;
            begin = end;
        } else {
            begin += 1;
        }
    }
    changed
}

/// Exact port of `SaveNoDrawsRestoreNooper` single pass over `op_types`.
fn pass_save_no_draws_restore(op_types: &mut [u32], noop_mask: &mut [bool]) -> bool {
    let count = op_types.len();
    let mut changed = false;
    let mut end = 0usize;

    let mut begin = end;
    while begin < count {
        let matched_end = if op_types[begin] == RecordType::Save as u32 {
            let mut i = begin + 1;
            while i < count {
                let t = op_types[i];
                let forbidden = t == RecordType::Save as u32
                    || t == RecordType::SaveLayer as u32
                    || t == RecordType::Restore as u32
                    || is_draw_type(t);
                if forbidden {
                    break;
                }
                i += 1;
            }
            if i < count && op_types[i] == RecordType::Restore as u32 {
                i + 1
            } else {
                0
            }
        } else {
            0
        };

        if matched_end != 0 {
            end = matched_end;
            for k in begin..end {
                op_types[k] = RecordType::NoOp as u32;
                noop_mask[k] = true;
            }
            changed = true;
            begin = end;
        } else {
            begin += 1;
        }
    }
    changed
}

/// Runs the fixed-point `SkRecordNoopSaveRestores` loop over `op_types`.
/// Writes `true` into `noop_mask[i]` for every op index that must be replaced with `NoOp`.
pub fn run_noop_save_restores(op_types: &mut [u32], noop_mask: &mut [bool]) {
    debug_assert_eq!(op_types.len(), noop_mask.len());
    for flag in noop_mask.iter_mut() {
        *flag = false;
    }
    while pass_save_only_draws_restore(op_types, noop_mask)
        || pass_save_no_draws_restore(op_types, noop_mask)
    {}
}

/// Finds all matches for `Pattern<Is<SaveLayer>, IsSingleDraw, Is<Restore>>`
/// using exact `Pattern::search` cursor advancement semantics.
/// Returns the starting indices (`begin`) of each match.
pub fn find_save_layer_draw_restore_matches(op_types: &[u32]) -> Vec<i32> {
    let count = op_types.len();
    let mut matches = Vec::new();
    let mut begin = 0usize;
    while begin < count {
        if begin + 2 < count
            && op_types[begin] == RecordType::SaveLayer as u32
            && is_single_draw_type(op_types[begin + 1])
            && op_types[begin + 2] == RecordType::Restore as u32
        {
            matches.push(begin as i32);
            begin += 3;
        } else {
            begin += 1;
        }
    }
    matches
}

/// Finds all matches for
/// `Pattern<Is<SaveLayer>, Is<Save>, Is<ClipRect>, Is<SaveLayer>, Is<Restore>, Is<Restore>, Is<Restore>>`
/// using exact `Pattern::search` cursor advancement semantics.
pub fn find_svg_opacity_filter_matches(op_types: &[u32]) -> Vec<i32> {
    let count = op_types.len();
    let mut matches = Vec::new();
    let mut begin = 0usize;
    while begin < count {
        if begin + 6 < count
            && op_types[begin] == RecordType::SaveLayer as u32
            && op_types[begin + 1] == RecordType::Save as u32
            && op_types[begin + 2] == RecordType::ClipRect as u32
            && op_types[begin + 3] == RecordType::SaveLayer as u32
            && op_types[begin + 4] == RecordType::Restore as u32
            && op_types[begin + 5] == RecordType::Restore as u32
            && op_types[begin + 6] == RecordType::Restore as u32
        {
            matches.push(begin as i32);
            begin += 7;
        } else {
            begin += 1;
        }
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mul_div_255_round() {
        assert_eq!(mul_div_255_round(0, 255), 0);
        assert_eq!(mul_div_255_round(255, 255), 255);
        assert_eq!(mul_div_255_round(128, 128), 64);
        assert_eq!(mul_div_255_round(255, 127), 127);
        assert_eq!(mul_div_255_round(200, 100), 78);
    }

    #[test]
    fn test_noop_save_restores_nested() {
        // Save, Save, ClipRect, Restore, DrawRect, Restore
        // Inner Save-ClipRect-Restore -> all NoOp
        // Outer Save-NoOp*3-DrawRect-Restore -> outer Save & Restore NoOp, DrawRect kept!
        let mut ops = [
            RecordType::Save as u32,
            RecordType::Save as u32,
            RecordType::ClipRect as u32,
            RecordType::Restore as u32,
            RecordType::DrawRect as u32,
            RecordType::Restore as u32,
        ];
        let mut mask = [false; 6];
        run_noop_save_restores(&mut ops, &mut mask);
        assert_eq!(mask, [true, true, true, true, false, true]);
    }

    #[test]
    fn test_save_layer_draw_restore_pattern_and_eval() {
        let ops = [
            RecordType::SaveLayer as u32,
            RecordType::DrawRect as u32,
            RecordType::Restore as u32,
            RecordType::SaveLayer as u32,
            RecordType::DrawPoints as u32, // MultiDraw -> not matched
            RecordType::Restore as u32,
            RecordType::SaveLayer as u32,
            RecordType::DrawOval as u32,
            RecordType::Restore as u32,
        ];
        assert_eq!(find_save_layer_draw_restore_matches(&ops), vec![0, 6]);

        let layer_paint = PaintOptInfo {
            present: true,
            is_src_over: true,
            alpha: 0x80,
            color: 0x8000_0000,
            ..Default::default()
        };
        let draw_paint = PaintOptInfo {
            present: true,
            is_src_over: true,
            alpha: 0xFF,
            color: 0xFFFF_0000,
            ..Default::default()
        };
        let dec = eval_save_layer_draw_restore(false, false, &layer_paint, &draw_paint);
        assert!(dec.kill_save_layer_and_restore);
        assert!(dec.update_paint_alpha);
        assert_eq!(dec.new_alpha, 0x80);

        // Non-zero RGB on layer paint disallows folding
        let bad_layer = PaintOptInfo {
            color: 0x8011_0000,
            ..layer_paint
        };
        assert!(!eval_save_layer_draw_restore(false, false, &bad_layer, &draw_paint)
            .kill_save_layer_and_restore);

        // Null layer paint + effectively srcover (opaque kSrc with no effects)
        let null_paint = PaintOptInfo::default();
        let src_opaque = PaintOptInfo {
            present: true,
            is_src_over: false,
            is_src_mode: true,
            alpha: 0xFF,
            color: 0xFF11_2233,
            ..Default::default()
        };
        let dec2 = eval_save_layer_draw_restore(false, false, &null_paint, &src_opaque);
        assert!(dec2.kill_save_layer_and_restore);
        assert!(!dec2.update_paint_alpha);
    }

    #[test]
    fn test_svg_opacity_filter_pattern_and_eval() {
        let ops = [
            RecordType::SaveLayer as u32,
            RecordType::Save as u32,
            RecordType::ClipRect as u32,
            RecordType::SaveLayer as u32,
            RecordType::Restore as u32,
            RecordType::Restore as u32,
            RecordType::Restore as u32,
        ];
        assert_eq!(find_svg_opacity_filter_matches(&ops), vec![0]);

        let opacity_paint = PaintOptInfo {
            present: true,
            is_src_over: true,
            alpha: 128,
            color: 0x8000_0000,
            ..Default::default()
        };
        let filter_paint = PaintOptInfo {
            present: true,
            is_src_over: true,
            has_image_filter: true, // allowed when is_save_layer == true!
            alpha: 200,
            color: 0xC800_0000,
            ..Default::default()
        };
        let dec = eval_svg_opacity_filter_merge(false, false, false, &opacity_paint, &filter_paint);
        assert!(dec.kill_save_layer_and_restore);
        assert!(dec.update_paint_alpha);
        assert_eq!(dec.new_alpha, mul_div_255_round(200, 128));
    }
}
