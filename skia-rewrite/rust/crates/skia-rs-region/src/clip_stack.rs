#![forbid(unsafe_code)]

//! Safe Rust port of `src/core/SkClipStack.cpp` and `src/utils/SkClipStackUtils.cpp`.
//!
//! Implements clip stack generation IDs, incremental finite/inside-out bounds
//! tracking, rect-rect in-place merging rules, conservative bounds, quick
//! containment, `isRRect`, and `SkClipStack_AsPath` step classification.

use crate::{make_large_s32, ClipOp};
use skia_rs_geometry::{IRect, Matrix, RRect, Rect};
use std::sync::atomic::{AtomicU32, Ordering};

/// Reserved generation IDs matching `SkClipStack::k*GenID`.
pub const INVALID_GEN_ID: u32 = 0;
pub const EMPTY_GEN_ID: u32 = 1;
pub const WIDE_OPEN_GEN_ID: u32 = 2;
pub const FIRST_UNRESERVED_GEN_ID: u32 = 3;

/// Matches `SkClipStack::GetNextGenID()`.
pub fn get_next_gen_id() -> u32 {
    static NEXT_ID: AtomicU32 = AtomicU32::new(FIRST_UNRESERVED_GEN_ID);
    loop {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        if id >= FIRST_UNRESERVED_GEN_ID {
            return id;
        }
    }
}

/// Matches `SkClipStack::BoundsType`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BoundsType {
    Normal = 0,
    InsideOut = 1,
}

impl BoundsType {
    #[inline]
    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Normal),
            1 => Some(Self::InsideOut),
            _ => None,
        }
    }
}

/// Matches `SkClipStack::Element::DeviceSpaceType`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DeviceSpaceType {
    Empty = 0,
    Rect = 1,
    RRect = 2,
    Path = 3,
    Shader = 4,
}

impl DeviceSpaceType {
    #[inline]
    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Empty),
            1 => Some(Self::Rect),
            2 => Some(Self::RRect),
            3 => Some(Self::Path),
            4 => Some(Self::Shader),
            _ => None,
        }
    }
}

/// Matches `SkClipStack::Element::FillCombo`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FillCombo {
    PrevCur = 0,
    PrevInvCur = 1,
    InvPrevCur = 2,
    InvPrevInvCur = 3,
}

impl FillCombo {
    #[inline]
    pub fn from_types(prev_type: BoundsType, cur_type: BoundsType) -> Self {
        let mut bits = 0i32;
        if cur_type == BoundsType::InsideOut {
            bits |= 0x01;
        }
        if prev_type == BoundsType::InsideOut {
            bits |= 0x02;
        }
        match bits {
            0 => Self::PrevCur,
            1 => Self::PrevInvCur,
            2 => Self::InvPrevCur,
            _ => Self::InvPrevInvCur,
        }
    }
}

/// Action decided by `decide_push` (matches `SkClipStack::pushElement`).
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PushAction {
    /// Prior is empty and can absorb the new clip in place: do nothing.
    NoOp = 0,
    /// Prior and new are both shaders and can be blended in place.
    MergeShaders = 1,
    /// Prior and new are both rects and can be intersected in place into `merged_rect`.
    MergeRects = 2,
    /// Prior becomes empty (disjoint bounds or empty rect intersection).
    SetPriorEmpty = 3,
    /// Element is a replace op: restore to `save_count - 1` before pushing.
    ReplaceAndPush = 4,
    /// Standard push onto the back of the deque.
    Push = 5,
}

/// Full decision returned by `decide_push`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PushDecision {
    pub action: PushAction,
    pub merged_rect: Rect,
}

/// Matches `SkClipStack::Element::canBeIntersectedInPlace`.
#[inline]
pub fn can_be_intersected_in_place(
    prior_type: DeviceSpaceType,
    prior_save_count: i32,
    prior_op: ClipOp,
    prior_is_replace: bool,
    save_count: i32,
    op: ClipOp,
) -> bool {
    if prior_type == DeviceSpaceType::Empty
        && (op == ClipOp::Difference || op == ClipOp::Intersect)
    {
        return true;
    }
    prior_save_count == save_count
        && op == ClipOp::Intersect
        && (prior_op == ClipOp::Intersect || prior_is_replace)
}

/// Matches `SkClipStack::Element::rectRectIntersectAllowed`.
#[inline]
pub fn rect_rect_intersect_allowed(
    existing_rect: Rect,
    existing_aa: bool,
    new_rect: Rect,
    new_aa: bool,
) -> bool {
    if existing_aa == new_aa {
        return true;
    }
    if !Rect::intersects(existing_rect, new_rect) {
        return true;
    }
    if existing_rect.contains_rect(new_rect) {
        return true;
    }
    false
}

/// Matches `SkClipStack::Element::combineBoundsDiff`.
pub fn combine_bounds_diff(
    combination: FillCombo,
    prev_finite: Rect,
    finite_bound: &mut Rect,
    finite_bound_type: &mut BoundsType,
    gen_id: &mut u32,
) {
    match combination {
        FillCombo::InvPrevInvCur => {
            *finite_bound_type = BoundsType::Normal;
        }
        FillCombo::InvPrevCur => {
            finite_bound.join(prev_finite);
            *finite_bound_type = BoundsType::InsideOut;
        }
        FillCombo::PrevInvCur => {
            if !finite_bound.intersect(prev_finite) {
                *finite_bound = Rect::EMPTY;
                *gen_id = EMPTY_GEN_ID;
            }
            *finite_bound_type = BoundsType::Normal;
        }
        FillCombo::PrevCur => {
            *finite_bound = prev_finite;
        }
    }
}

/// Matches `SkClipStack::Element::combineBoundsIntersection`.
/// Returns `true` if the intersection became empty (caller should `set_empty()`).
pub fn combine_bounds_intersection(
    combination: FillCombo,
    prev_finite: Rect,
    finite_bound: &mut Rect,
    finite_bound_type: &mut BoundsType,
) -> bool {
    match combination {
        FillCombo::InvPrevInvCur => {
            finite_bound.join(prev_finite);
            *finite_bound_type = BoundsType::InsideOut;
            false
        }
        FillCombo::InvPrevCur => false,
        FillCombo::PrevInvCur => {
            *finite_bound = prev_finite;
            *finite_bound_type = BoundsType::Normal;
            false
        }
        FillCombo::PrevCur => !finite_bound.intersect(prev_finite),
    }
}

/// Result of `update_bound_and_gen_id`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct BoundUpdateResult {
    pub finite_bound: Rect,
    pub finite_bound_type: BoundsType,
    pub is_intersection_of_rects: bool,
    pub gen_id: u32,
    pub became_empty: bool,
}

/// Prior element summary passed to `update_bound_and_gen_id`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PriorBoundInfo {
    pub finite_bound: Rect,
    pub finite_bound_type: BoundsType,
    pub is_intersection_of_rects: bool,
    pub device_space_rect: Rect,
    pub do_aa: bool,
}

/// Matches `SkClipStack::Element::updateBoundAndGenID(const Element* prior)`.
pub fn compute_bound_and_gen_id(
    cur_type: DeviceSpaceType,
    cur_bounds: Rect,
    cur_op: ClipOp,
    cur_do_aa: bool,
    cur_is_replace: bool,
    cur_path_is_inverse: bool,
    prior: Option<PriorBoundInfo>,
) -> BoundUpdateResult {
    let mut gen_id = get_next_gen_id();
    let mut is_intersection_of_rects = false;
    let mut finite_bound;
    let mut finite_bound_type;

    match cur_type {
        DeviceSpaceType::Rect => {
            finite_bound = cur_bounds;
            finite_bound_type = BoundsType::Normal;
            if cur_is_replace
                || (cur_op == ClipOp::Intersect && prior.is_none())
                || (cur_op == ClipOp::Intersect
                    && prior.map_or(false, |p| {
                        p.is_intersection_of_rects
                            && rect_rect_intersect_allowed(
                                p.device_space_rect,
                                p.do_aa,
                                cur_bounds,
                                cur_do_aa,
                            )
                    }))
            {
                is_intersection_of_rects = true;
            }
        }
        DeviceSpaceType::RRect => {
            finite_bound = cur_bounds;
            finite_bound_type = BoundsType::Normal;
        }
        DeviceSpaceType::Path => {
            finite_bound = cur_bounds;
            finite_bound_type = if cur_path_is_inverse {
                BoundsType::InsideOut
            } else {
                BoundsType::Normal
            };
        }
        DeviceSpaceType::Shader => {
            finite_bound = make_large_s32();
            finite_bound_type = BoundsType::Normal;
        }
        DeviceSpaceType::Empty => {
            return BoundUpdateResult {
                finite_bound: Rect::EMPTY,
                finite_bound_type: BoundsType::Normal,
                is_intersection_of_rects: false,
                gen_id: EMPTY_GEN_ID,
                became_empty: true,
            };
        }
    }

    let (prev_finite, prev_type) = match prior {
        None => (Rect::EMPTY, BoundsType::InsideOut),
        Some(p) => (p.finite_bound, p.finite_bound_type),
    };

    let combination = FillCombo::from_types(prev_type, finite_bound_type);
    let mut became_empty = false;

    if !cur_is_replace {
        match cur_op {
            ClipOp::Difference => {
                combine_bounds_diff(
                    combination,
                    prev_finite,
                    &mut finite_bound,
                    &mut finite_bound_type,
                    &mut gen_id,
                );
            }
            ClipOp::Intersect => {
                if combine_bounds_intersection(
                    combination,
                    prev_finite,
                    &mut finite_bound,
                    &mut finite_bound_type,
                ) {
                    became_empty = true;
                    finite_bound = Rect::EMPTY;
                    finite_bound_type = BoundsType::Normal;
                    is_intersection_of_rects = false;
                    gen_id = EMPTY_GEN_ID;
                }
            }
        }
    }

    BoundUpdateResult {
        finite_bound,
        finite_bound_type,
        is_intersection_of_rects,
        gen_id,
        became_empty,
    }
}

/// Decides what `SkClipStack::pushElement` should do given the current back
/// element (if any) and the incoming element's properties.
#[allow(clippy::too_many_arguments)]
pub fn decide_push(
    has_prior: bool,
    prior_type: DeviceSpaceType,
    prior_save_count: i32,
    prior_op: ClipOp,
    prior_is_replace: bool,
    prior_do_aa: bool,
    prior_bounds: Rect,
    save_count: i32,
    new_type: DeviceSpaceType,
    new_op: ClipOp,
    new_is_replace: bool,
    new_do_aa: bool,
    new_bounds: Rect,
) -> PushDecision {
    if !has_prior {
        return PushDecision {
            action: PushAction::Push,
            merged_rect: Rect::EMPTY,
        };
    }

    if new_is_replace {
        return PushDecision {
            action: PushAction::ReplaceAndPush,
            merged_rect: Rect::EMPTY,
        };
    }

    if can_be_intersected_in_place(
        prior_type,
        prior_save_count,
        prior_op,
        prior_is_replace,
        save_count,
        new_op,
    ) {
        match prior_type {
            DeviceSpaceType::Empty => {
                return PushDecision {
                    action: PushAction::NoOp,
                    merged_rect: Rect::EMPTY,
                };
            }
            DeviceSpaceType::Shader => {
                if new_type == DeviceSpaceType::Shader {
                    return PushDecision {
                        action: PushAction::MergeShaders,
                        merged_rect: Rect::EMPTY,
                    };
                }
            }
            DeviceSpaceType::Rect => {
                if new_type == DeviceSpaceType::Rect {
                    if rect_rect_intersect_allowed(prior_bounds, prior_do_aa, new_bounds, new_do_aa)
                    {
                        let mut isect_rect = Rect::EMPTY;
                        if !isect_rect.intersect_pair(prior_bounds, new_bounds) {
                            return PushDecision {
                                action: PushAction::SetPriorEmpty,
                                merged_rect: Rect::EMPTY,
                            };
                        }
                        return PushDecision {
                            action: PushAction::MergeRects,
                            merged_rect: isect_rect,
                        };
                    }
                } else if !Rect::intersects(prior_bounds, new_bounds) {
                    return PushDecision {
                        action: PushAction::SetPriorEmpty,
                        merged_rect: Rect::EMPTY,
                    };
                }
            }
            DeviceSpaceType::RRect | DeviceSpaceType::Path => {
                if !Rect::intersects(prior_bounds, new_bounds) {
                    return PushDecision {
                        action: PushAction::SetPriorEmpty,
                        merged_rect: Rect::EMPTY,
                    };
                }
            }
        }
    }

    PushDecision {
        action: PushAction::Push,
        merged_rect: Rect::EMPTY,
    }
}

/// Matches `SkClipStack::bounds(const SkIRect& deviceBounds)`.
pub fn compute_clipped_bounds(
    finite_bound: Rect,
    bound_type: BoundsType,
    device_bounds: IRect,
) -> Rect {
    let dev = Rect::from_irect(device_bounds);
    if bound_type == BoundsType::InsideOut {
        return dev;
    }
    let mut r = finite_bound;
    if r.intersect(dev) {
        r
    } else {
        Rect::EMPTY
    }
}

/// Matches `SkClipStack::getConservativeBounds(...)`.
pub fn compute_conservative_bounds(
    finite_bound: Rect,
    bound_type: BoundsType,
    offset_x: i32,
    offset_y: i32,
    max_width: i32,
    max_height: i32,
) -> Rect {
    let mut dev_bounds = Rect::from_ltrb(0.0, 0.0, max_width as f32, max_height as f32);
    if bound_type == BoundsType::InsideOut {
        return dev_bounds;
    }
    let temp = finite_bound.make_offset(offset_x as f32, offset_y as f32);
    if !dev_bounds.intersect(temp) {
        dev_bounds = Rect::EMPTY;
    }
    dev_bounds
}

/// Matches `SkClipStack::getTopmostGenID()`.
#[inline]
pub fn compute_topmost_gen_id(
    is_empty_deque: bool,
    back_type: DeviceSpaceType,
    back_bound_type: BoundsType,
    back_finite_bound: Rect,
    back_gen_id: u32,
) -> u32 {
    if is_empty_deque {
        return WIDE_OPEN_GEN_ID;
    }
    if back_bound_type == BoundsType::InsideOut
        && back_finite_bound.is_empty()
        && back_type != DeviceSpaceType::Shader
    {
        return WIDE_OPEN_GEN_ID;
    }
    back_gen_id
}

/// Action for a single element during `SkClipStack_AsPath` (`src/utils/SkClipStackUtils.cpp`).
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AsPathStepAction {
    /// Shader elements are skipped.
    Skip = 0,
    /// Replace current path with empty path (`isReplace && kEmpty`).
    ReplaceEmpty = 1,
    /// Replace current path with `element.asDeviceSpacePath()`.
    ReplaceGeometry = 2,
    /// Combine current path with empty path using `element.getOp()`.
    OpEmpty = 3,
    /// Combine current path with `element.asDeviceSpacePath()` using `element.getOp()`.
    OpGeometry = 4,
}

/// Matches the per-element dispatch in `SkClipStack_AsPath`.
#[inline]
pub fn classify_as_path_step(dev_type: DeviceSpaceType, is_replace: bool) -> AsPathStepAction {
    if dev_type == DeviceSpaceType::Shader {
        return AsPathStepAction::Skip;
    }
    match (is_replace, dev_type == DeviceSpaceType::Empty) {
        (true, true) => AsPathStepAction::ReplaceEmpty,
        (true, false) => AsPathStepAction::ReplaceGeometry,
        (false, true) => AsPathStepAction::OpEmpty,
        (false, false) => AsPathStepAction::OpGeometry,
    }
}

// ---------------------------------------------------------------------------
// Pure safe Rust `ClipStackElement` & `ClipStack`
// ---------------------------------------------------------------------------

/// Safe Rust representation of `SkClipStack::Element` (geometry + bounds/genID state).
#[derive(Clone, Debug, PartialEq)]
pub struct ClipStackElement {
    pub rrect: RRect,
    pub path_bounds: Rect,
    pub path_is_inverse: bool,
    pub shader_id: u64,
    pub save_count: i32,
    pub op: ClipOp,
    pub device_space_type: DeviceSpaceType,
    pub do_aa: bool,
    pub is_replace: bool,
    pub finite_bound_type: BoundsType,
    pub finite_bound: Rect,
    pub is_intersection_of_rects: bool,
    pub gen_id: u32,
}

impl ClipStackElement {
    fn init_common(save_count: i32, op: ClipOp, do_aa: bool) -> Self {
        Self {
            rrect: RRect::make_empty(),
            path_bounds: Rect::EMPTY,
            path_is_inverse: false,
            shader_id: 0,
            save_count,
            op,
            device_space_type: DeviceSpaceType::Empty,
            do_aa,
            is_replace: false,
            finite_bound_type: BoundsType::InsideOut,
            finite_bound: Rect::EMPTY,
            is_intersection_of_rects: false,
            gen_id: INVALID_GEN_ID,
        }
    }

    pub fn new_empty(save_count: i32) -> Self {
        let mut el = Self::init_common(save_count, ClipOp::Intersect, false);
        el.set_empty();
        el
    }

    pub fn new_rect(save_count: i32, rect: Rect, matrix: &Matrix, op: ClipOp, do_aa: bool) -> Self {
        let mut el = Self::init_common(save_count, op, do_aa);
        if matrix.rect_stays_rect() {
            let dev_rect = if let Some((r, _)) = matrix.map_rect_non_perspective(rect) {
                r
            } else {
                matrix.map_rect_scale_translate(rect)
            };
            el.rrect.set_rect(dev_rect);
            el.device_space_type = DeviceSpaceType::Rect;
        } else {
            let pts = rect.to_quad();
            let mut mapped = [skia_rs_geometry::Point::default(); 4];
            matrix.map_points(&mut mapped, &pts);
            el.path_bounds = Rect::bounds_or_empty(&mapped);
            el.device_space_type = DeviceSpaceType::Path;
        }
        el
    }

    pub fn new_rrect(save_count: i32, rrect: RRect, op: ClipOp, do_aa: bool) -> Self {
        let mut el = Self::init_common(save_count, op, do_aa);
        el.rrect = rrect;
        el.device_space_type = if rrect.rtype == skia_rs_geometry::rrect::TYPE_RECT
            || rrect.rtype == skia_rs_geometry::rrect::TYPE_EMPTY
        {
            DeviceSpaceType::Rect
        } else {
            DeviceSpaceType::RRect
        };
        el
    }

    pub fn new_replace_rect(save_count: i32, rect: Rect, do_aa: bool) -> Self {
        let mut el = Self::init_common(save_count, ClipOp::Intersect, do_aa);
        el.rrect.set_rect(rect);
        el.device_space_type = DeviceSpaceType::Rect;
        el.is_replace = true;
        el
    }

    pub fn new_path_bounds(
        save_count: i32,
        bounds: Rect,
        is_inverse: bool,
        op: ClipOp,
        do_aa: bool,
    ) -> Self {
        let mut el = Self::init_common(save_count, op, do_aa);
        el.path_bounds = bounds;
        el.path_is_inverse = is_inverse;
        el.device_space_type = DeviceSpaceType::Path;
        el
    }

    pub fn set_empty(&mut self) {
        self.device_space_type = DeviceSpaceType::Empty;
        self.finite_bound = Rect::EMPTY;
        self.finite_bound_type = BoundsType::Normal;
        self.is_intersection_of_rects = false;
        self.rrect = RRect::make_empty();
        self.path_bounds = Rect::EMPTY;
        self.path_is_inverse = false;
        self.shader_id = 0;
        self.gen_id = EMPTY_GEN_ID;
    }

    pub fn bounds(&self) -> Rect {
        match self.device_space_type {
            DeviceSpaceType::Rect | DeviceSpaceType::RRect => self.rrect.rect,
            DeviceSpaceType::Path => self.path_bounds,
            DeviceSpaceType::Shader => make_large_s32(),
            DeviceSpaceType::Empty => Rect::EMPTY,
        }
    }

    pub fn is_inverse_filled(&self) -> bool {
        self.device_space_type == DeviceSpaceType::Path && self.path_is_inverse
    }

    pub fn contains_rect(&self, rect: Rect) -> bool {
        match self.device_space_type {
            DeviceSpaceType::Rect => self.rrect.rect.contains_rect(rect),
            DeviceSpaceType::RRect => self.rrect.contains_rect(rect),
            DeviceSpaceType::Path | DeviceSpaceType::Empty | DeviceSpaceType::Shader => false,
        }
    }

    pub fn contains_rrect(&self, rrect: &RRect) -> bool {
        match self.device_space_type {
            DeviceSpaceType::Rect => self.rrect.rect.contains_rect(rrect.rect),
            DeviceSpaceType::RRect => {
                self.rrect.contains_rect(rrect.rect) || *rrect == self.rrect
            }
            DeviceSpaceType::Path | DeviceSpaceType::Empty | DeviceSpaceType::Shader => false,
        }
    }

    fn prior_info(&self) -> PriorBoundInfo {
        PriorBoundInfo {
            finite_bound: self.finite_bound,
            finite_bound_type: self.finite_bound_type,
            is_intersection_of_rects: self.is_intersection_of_rects,
            device_space_rect: self.rrect.rect,
            do_aa: self.do_aa,
        }
    }

    pub fn update_bound_and_gen_id(&mut self, prior: Option<&ClipStackElement>) {
        let res = compute_bound_and_gen_id(
            self.device_space_type,
            self.bounds(),
            self.op,
            self.do_aa,
            self.is_replace,
            self.path_is_inverse,
            prior.map(|p| p.prior_info()),
        );
        if res.became_empty {
            self.set_empty();
        } else {
            self.finite_bound = res.finite_bound;
            self.finite_bound_type = res.finite_bound_type;
            self.is_intersection_of_rects = res.is_intersection_of_rects;
            self.gen_id = res.gen_id;
        }
    }
}

/// Safe Rust port of `SkClipStack`.
#[derive(Clone, Debug, Default)]
pub struct ClipStack {
    elements: Vec<ClipStackElement>,
    save_count: i32,
}

impl PartialEq for ClipStack {
    fn eq(&self, other: &Self) -> bool {
        if self.get_topmost_gen_id() == other.get_topmost_gen_id() {
            return true;
        }
        if self.save_count != other.save_count || self.elements.len() != other.elements.len() {
            return false;
        }
        self.elements.iter().zip(other.elements.iter()).all(|(a, b)| {
            a.op == b.op
                && a.device_space_type == b.device_space_type
                && a.do_aa == b.do_aa
                && a.is_replace == b.is_replace
                && a.save_count == b.save_count
                && match a.device_space_type {
                    DeviceSpaceType::Empty => true,
                    DeviceSpaceType::Rect => a.rrect.rect == b.rrect.rect,
                    DeviceSpaceType::RRect => a.rrect == b.rrect,
                    DeviceSpaceType::Path => {
                        a.path_bounds == b.path_bounds && a.path_is_inverse == b.path_is_inverse
                    }
                    DeviceSpaceType::Shader => a.shader_id == b.shader_id,
                }
        })
    }
}

impl ClipStack {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.elements.clear();
        self.save_count = 0;
    }

    #[inline]
    pub fn save_count(&self) -> i32 {
        self.save_count
    }

    #[inline]
    pub fn elements(&self) -> &[ClipStackElement] {
        &self.elements
    }

    pub fn save(&mut self) {
        self.save_count += 1;
    }

    pub fn restore(&mut self) {
        self.save_count -= 1;
        self.restore_to(self.save_count);
    }

    fn restore_to(&mut self, save_count: i32) {
        while let Some(back) = self.elements.last() {
            if back.save_count <= save_count {
                break;
            }
            self.elements.pop();
        }
    }

    pub fn get_bounds(&self) -> (Rect, BoundsType, bool) {
        match self.elements.last() {
            None => (Rect::EMPTY, BoundsType::InsideOut, false),
            Some(el) => (
                el.finite_bound,
                el.finite_bound_type,
                el.is_intersection_of_rects,
            ),
        }
    }

    pub fn bounds(&self, device_bounds: IRect) -> Rect {
        let (r, bt, _) = self.get_bounds();
        compute_clipped_bounds(r, bt, device_bounds)
    }

    pub fn is_empty(&self, device_bounds: IRect) -> bool {
        self.bounds(device_bounds).is_empty()
    }

    pub fn get_conservative_bounds(
        &self,
        offset_x: i32,
        offset_y: i32,
        max_width: i32,
        max_height: i32,
    ) -> (Rect, bool) {
        let (r, bt, is_rect_isect) = self.get_bounds();
        (
            compute_conservative_bounds(r, bt, offset_x, offset_y, max_width, max_height),
            is_rect_isect,
        )
    }

    pub fn get_topmost_gen_id(&self) -> u32 {
        match self.elements.last() {
            None => WIDE_OPEN_GEN_ID,
            Some(back) => compute_topmost_gen_id(
                false,
                back.device_space_type,
                back.finite_bound_type,
                back.finite_bound,
                back.gen_id,
            ),
        }
    }

    pub fn is_wide_open(&self) -> bool {
        self.get_topmost_gen_id() == WIDE_OPEN_GEN_ID
    }

    pub fn quick_contains_rect(&self, dev_rect: Rect) -> bool {
        if self.is_wide_open() {
            return true;
        }
        for el in self.elements.iter().rev() {
            if el.op != ClipOp::Intersect && !el.is_replace {
                return false;
            }
            if el.is_inverse_filled() {
                if Rect::intersects(el.bounds(), dev_rect) {
                    return false;
                }
            } else if !el.contains_rect(dev_rect) {
                return false;
            }
            if el.is_replace {
                break;
            }
        }
        true
    }

    pub fn quick_contains_rrect(&self, dev_rrect: &RRect) -> bool {
        if self.is_wide_open() {
            return true;
        }
        for el in self.elements.iter().rev() {
            if el.op != ClipOp::Intersect && !el.is_replace {
                return false;
            }
            if el.is_inverse_filled() {
                if Rect::intersects(el.bounds(), dev_rrect.rect) {
                    return false;
                }
            } else if !el.contains_rrect(dev_rrect) {
                return false;
            }
            if el.is_replace {
                break;
            }
        }
        true
    }

    pub fn is_rrect(&self, bounds: Rect) -> Option<(RRect, bool)> {
        let back = self.elements.last()?;
        if back.is_intersection_of_rects && back.finite_bound_type == BoundsType::Normal {
            return Some((RRect::make_rect(back.finite_bound), back.do_aa));
        }
        if back.device_space_type != DeviceSpaceType::Rect
            && back.device_space_type != DeviceSpaceType::RRect
        {
            return None;
        }
        if back.is_replace {
            return Some((back.rrect, back.do_aa));
        }
        if back.op == ClipOp::Intersect {
            let mut back_bounds = Rect::EMPTY;
            if !back_bounds.intersect_pair(bounds, back.rrect.rect) {
                return None;
            }
            let cnt = self.elements.len();
            if cnt > 17 {
                return None;
            }
            if cnt > 1 {
                for prior in self.elements[..cnt - 1].iter().rev() {
                    if (prior.op != ClipOp::Intersect && !prior.is_replace)
                        || !prior.contains_rect(back_bounds)
                    {
                        return None;
                    }
                    if prior.is_replace {
                        break;
                    }
                }
            }
            return Some((back.rrect, back.do_aa));
        }
        None
    }

    pub fn clip_rect(&mut self, rect: Rect, matrix: &Matrix, op: ClipOp, do_aa: bool) {
        let el = ClipStackElement::new_rect(self.save_count, rect, matrix, op, do_aa);
        self.push_element(el);
    }

    pub fn clip_rrect(&mut self, rrect: RRect, op: ClipOp, do_aa: bool) {
        let el = ClipStackElement::new_rrect(self.save_count, rrect, op, do_aa);
        self.push_element(el);
    }

    pub fn replace_clip(&mut self, dev_rect: Rect, do_aa: bool) {
        let el = ClipStackElement::new_replace_rect(self.save_count, dev_rect, do_aa);
        self.push_element(el);
    }

    pub fn clip_empty(&mut self) {
        if let Some(el) = self.elements.last_mut() {
            if can_be_intersected_in_place(
                el.device_space_type,
                el.save_count,
                el.op,
                el.is_replace,
                self.save_count,
                ClipOp::Intersect,
            ) {
                el.set_empty();
            }
        }
        let mut el = ClipStackElement::new_empty(self.save_count);
        el.gen_id = EMPTY_GEN_ID;
        self.elements.push(el);
    }

    pub fn push_element(&mut self, mut element: ClipStackElement) {
        let decision = match self.elements.last() {
            None => PushDecision {
                action: PushAction::Push,
                merged_rect: Rect::EMPTY,
            },
            Some(prior) => decide_push(
                true,
                prior.device_space_type,
                prior.save_count,
                prior.op,
                prior.is_replace,
                prior.do_aa,
                prior.bounds(),
                self.save_count,
                element.device_space_type,
                element.op,
                element.is_replace,
                element.do_aa,
                element.bounds(),
            ),
        };

        match decision.action {
            PushAction::NoOp => return,
            PushAction::SetPriorEmpty => {
                if let Some(prior) = self.elements.last_mut() {
                    prior.set_empty();
                }
                return;
            }
            PushAction::MergeRects => {
                let len = self.elements.len();
                let prior_prior = if len >= 2 {
                    Some(self.elements[len - 2].clone())
                } else {
                    None
                };
                let prior = self.elements.last_mut().unwrap();
                prior.rrect.set_rect(decision.merged_rect);
                prior.do_aa = element.do_aa;
                prior.update_bound_and_gen_id(prior_prior.as_ref());
                return;
            }
            PushAction::MergeShaders => {
                let len = self.elements.len();
                let prior_prior = if len >= 2 {
                    Some(self.elements[len - 2].clone())
                } else {
                    None
                };
                let prior = self.elements.last_mut().unwrap();
                prior.shader_id ^= element.shader_id.rotate_left(7);
                prior.update_bound_and_gen_id(prior_prior.as_ref());
                return;
            }
            PushAction::ReplaceAndPush => {
                self.restore_to(self.save_count - 1);
            }
            PushAction::Push => {}
        }

        let prior = self.elements.last().cloned();
        element.update_bound_and_gen_id(prior.as_ref());
        self.elements.push(element);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wide_open_and_save_restore() {
        let mut cs = ClipStack::new();
        assert!(cs.is_wide_open());
        assert_eq!(cs.get_topmost_gen_id(), WIDE_OPEN_GEN_ID);
        assert_eq!(
            cs.bounds(IRect::from_wh(100, 100)),
            Rect::from_ltrb(0.0, 0.0, 100.0, 100.0)
        );

        cs.save();
        cs.clip_rect(
            Rect::from_ltrb(10.0, 20.0, 80.0, 90.0),
            &Matrix::identity(),
            ClipOp::Intersect,
            false,
        );
        assert!(!cs.is_wide_open());
        assert!(cs.get_topmost_gen_id() >= FIRST_UNRESERVED_GEN_ID);
        assert_eq!(
            cs.bounds(IRect::from_wh(100, 100)),
            Rect::from_ltrb(10.0, 20.0, 80.0, 90.0)
        );
        assert!(cs.quick_contains_rect(Rect::from_ltrb(20.0, 30.0, 50.0, 60.0)));
        assert!(!cs.quick_contains_rect(Rect::from_ltrb(0.0, 0.0, 50.0, 60.0)));

        // Intersect in place merges within same save count
        cs.clip_rect(
            Rect::from_ltrb(30.0, 10.0, 60.0, 70.0),
            &Matrix::identity(),
            ClipOp::Intersect,
            false,
        );
        assert_eq!(cs.elements().len(), 1);
        assert_eq!(
            cs.bounds(IRect::from_wh(100, 100)),
            Rect::from_ltrb(30.0, 20.0, 60.0, 70.0)
        );

        let (rr, aa) = cs.is_rrect(Rect::from_ltrb(0.0, 0.0, 100.0, 100.0)).unwrap();
        assert_eq!(rr.rect, Rect::from_ltrb(30.0, 20.0, 60.0, 70.0));
        assert!(!aa);

        cs.restore();
        assert!(cs.is_wide_open());
        assert!(cs.elements().is_empty());
    }

    #[test]
    fn test_disjoint_becomes_empty() {
        let mut cs = ClipStack::new();
        cs.clip_rect(
            Rect::from_ltrb(0.0, 0.0, 10.0, 10.0),
            &Matrix::identity(),
            ClipOp::Intersect,
            false,
        );
        cs.clip_rect(
            Rect::from_ltrb(20.0, 20.0, 30.0, 30.0),
            &Matrix::identity(),
            ClipOp::Intersect,
            false,
        );
        assert_eq!(cs.get_topmost_gen_id(), EMPTY_GEN_ID);
        assert!(cs.is_empty(IRect::from_wh(100, 100)));
    }

    #[test]
    fn test_conservative_bounds_offset() {
        let mut cs = ClipStack::new();
        cs.clip_rect(
            Rect::from_ltrb(10.0, 10.0, 50.0, 50.0),
            &Matrix::identity(),
            ClipOp::Intersect,
            false,
        );
        let (b, is_rect) = cs.get_conservative_bounds(-5, -5, 100, 100);
        assert!(is_rect);
        assert_eq!(b, Rect::from_ltrb(5.0, 5.0, 45.0, 45.0));
    }

    #[test]
    fn test_replace_clip_and_difference() {
        let mut cs = ClipStack::new();
        cs.save();
        cs.clip_rect(
            Rect::from_ltrb(0.0, 0.0, 50.0, 50.0),
            &Matrix::identity(),
            ClipOp::Intersect,
            false,
        );
        cs.replace_clip(Rect::from_ltrb(60.0, 60.0, 90.0, 90.0), true);
        assert_eq!(cs.elements().len(), 1);
        assert_eq!(
            cs.bounds(IRect::from_wh(100, 100)),
            Rect::from_ltrb(60.0, 60.0, 90.0, 90.0)
        );
        let (rr, aa) = cs.is_rrect(Rect::from_ltrb(0.0, 0.0, 100.0, 100.0)).unwrap();
        assert_eq!(rr.rect, Rect::from_ltrb(60.0, 60.0, 90.0, 90.0));
        assert!(aa);
        cs.restore();
        assert!(cs.is_wide_open());
    }

    #[test]
    fn test_as_path_step_classification() {
        assert_eq!(
            classify_as_path_step(DeviceSpaceType::Shader, false),
            AsPathStepAction::Skip
        );
        assert_eq!(
            classify_as_path_step(DeviceSpaceType::Empty, true),
            AsPathStepAction::ReplaceEmpty
        );
        assert_eq!(
            classify_as_path_step(DeviceSpaceType::Rect, true),
            AsPathStepAction::ReplaceGeometry
        );
        assert_eq!(
            classify_as_path_step(DeviceSpaceType::Empty, false),
            AsPathStepAction::OpEmpty
        );
        assert_eq!(
            classify_as_path_step(DeviceSpaceType::Path, false),
            AsPathStepAction::OpGeometry
        );
    }
}
