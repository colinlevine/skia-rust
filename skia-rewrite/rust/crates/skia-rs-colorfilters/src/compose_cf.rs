//! Safe Rust port of `src/effects/colorfilters/SkComposeColorFilter.cpp`.

/// Exact port of `SkComposeColorFilter::onIsAlphaUnchanged()`.
/// Can only claim alpha-unchanged support if both outer and inner do.
#[inline]
pub const fn compose_is_alpha_unchanged(
    outer_alpha_unchanged: bool,
    inner_alpha_unchanged: bool,
) -> bool {
    outer_alpha_unchanged && inner_alpha_unchanged
}

/// Exact port of the `innerIsOpaque` calculation in `SkComposeColorFilter::appendStages`.
#[inline]
pub const fn compose_inner_is_opaque(
    shader_is_opaque: bool,
    inner_alpha_unchanged: bool,
) -> bool {
    if !inner_alpha_unchanged {
        false
    } else {
        shader_is_opaque
    }
}

/// Decision for `SkColorFilter::makeComposed(inner)` / `SkComposeColorFilter::CreateProc`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ComposeAction {
    /// Both `outer` and `inner` are null -> return `nullptr`.
    ReturnNull = 0,
    /// Only `outer` is non-null -> return `outer`.
    ReturnOuter = 1,
    /// Only `inner` is non-null -> return `inner`.
    ReturnInner = 2,
    /// Both `outer` and `inner` are non-null -> allocate `SkComposeColorFilter(outer, inner)`.
    CreateCompose = 3,
}

/// Classify composition of `(outer, inner)` based on presence of each filter.
#[inline]
pub const fn classify_compose(has_outer: bool, has_inner: bool) -> ComposeAction {
    match (has_outer, has_inner) {
        (false, false) => ComposeAction::ReturnNull,
        (true, false) => ComposeAction::ReturnOuter,
        (false, true) => ComposeAction::ReturnInner,
        (true, true) => ComposeAction::CreateCompose,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_and_opacity_propagation() {
        assert!(compose_is_alpha_unchanged(true, true));
        assert!(!compose_is_alpha_unchanged(true, false));
        assert!(!compose_is_alpha_unchanged(false, true));
        assert!(!compose_is_alpha_unchanged(false, false));

        assert!(compose_inner_is_opaque(true, true));
        assert!(!compose_inner_is_opaque(true, false));
        assert!(!compose_inner_is_opaque(false, true));
        assert!(!compose_inner_is_opaque(false, false));
    }

    #[test]
    fn compose_null_folding() {
        assert_eq!(classify_compose(false, false), ComposeAction::ReturnNull);
        assert_eq!(classify_compose(true, false), ComposeAction::ReturnOuter);
        assert_eq!(classify_compose(false, true), ComposeAction::ReturnInner);
        assert_eq!(classify_compose(true, true), ComposeAction::CreateCompose);
    }
}
