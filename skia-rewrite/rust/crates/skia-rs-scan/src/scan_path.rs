#![forbid(unsafe_code)]

//! Safe Rust port of `src/core/SkScan_Path.cpp`.

use crate::blitter::Blitter;
use crate::edge::{BasicEdge, EdgeBase};
use crate::edge_builder::BasicEdgeBuilder;
use crate::fixed::{fixed_round_to_int, sk_tqsort, SkFixed, SK_MAX_S32, SK_MIN_S32};
use skia_rs_geometry::{IRect, Point};
use skia_rs_path::{FillType, PathRaw};

const NONE: usize = usize::MAX;
const HEAD_IDX: usize = 0;
const TAIL_IDX: usize = 1;

const EDGE_HEAD_Y: i32 = SK_MIN_S32;
const EDGE_TAIL_Y: i32 = SK_MAX_S32;

struct EdgeNode {
    edge: BasicEdge,
    prev: usize,
    next: usize,
}

#[inline]
fn remove_edge(nodes: &mut [EdgeNode], idx: usize) {
    let prev = nodes[idx].prev;
    let next = nodes[idx].next;
    nodes[prev].next = next;
    nodes[next].prev = prev;
}

#[inline]
fn insert_edge_after(nodes: &mut [EdgeNode], idx: usize, after_me: usize) {
    let after_next = nodes[after_me].next;
    nodes[idx].prev = after_me;
    nodes[idx].next = after_next;
    nodes[after_next].prev = idx;
    nodes[after_me].next = idx;
}

#[inline]
fn backward_insert_start(nodes: &[EdgeNode], mut prev: usize, x: SkFixed) -> usize {
    while nodes[prev].prev != NONE && nodes[prev].edge.base().x > x {
        prev = nodes[prev].prev;
    }
    prev
}

#[inline]
fn backward_insert_edge_based_on_x(nodes: &mut [EdgeNode], idx: usize) {
    let x = nodes[idx].edge.base().x;
    let mut prev = nodes[idx].prev;
    while nodes[prev].prev != NONE && nodes[prev].edge.base().x > x {
        prev = nodes[prev].prev;
    }
    if nodes[prev].next != idx {
        remove_edge(nodes, idx);
        insert_edge_after(nodes, idx, prev);
    }
}

fn insert_new_edges(nodes: &mut [EdgeNode], mut new_edge: usize, curr_y: i32) {
    if nodes[new_edge].edge.base().first_y != curr_y {
        return;
    }
    let prev = nodes[new_edge].prev;
    if nodes[prev].edge.base().x <= nodes[new_edge].edge.base().x {
        return;
    }
    let mut start = backward_insert_start(nodes, prev, nodes[new_edge].edge.base().x);
    loop {
        let next = nodes[new_edge].next;
        let new_x = nodes[new_edge].edge.base().x;
        let mut need_move = true;
        loop {
            if nodes[start].next == new_edge {
                need_move = false;
                break;
            }
            let after = nodes[start].next;
            if nodes[after].edge.base().x >= new_x {
                break;
            }
            start = after;
        }
        if need_move {
            remove_edge(nodes, new_edge);
            insert_edge_after(nodes, new_edge, start);
        }
        start = new_edge;
        new_edge = next;
        if nodes[new_edge].edge.base().first_y != curr_y {
            break;
        }
    }
}

/// Helper matching `InverseBlitter` in `SkScan_Path.cpp`.
struct InverseState {
    first_x: i32,
    last_x: i32,
    prev_x: i32,
}

impl InverseState {
    fn new(clip: IRect) -> Self {
        Self {
            first_x: clip.left,
            last_x: clip.right,
            prev_x: clip.left,
        }
    }

    #[inline]
    fn pre(&mut self) {
        self.prev_x = self.first_x;
    }

    #[inline]
    fn post<B: Blitter + ?Sized>(&mut self, blitter: &mut B, y: i32) {
        let inv_width = self.last_x - self.prev_x;
        if inv_width > 0 {
            blitter.blit_h(self.prev_x, y, inv_width);
        }
    }

    #[inline]
    fn blit_h<B: Blitter + ?Sized>(&mut self, blitter: &mut B, x: i32, y: i32, width: i32) {
        let inv_width = x - self.prev_x;
        if inv_width > 0 {
            blitter.blit_h(self.prev_x, y, inv_width);
        }
        self.prev_x = x + width;
    }
}

fn walk_edges<B: Blitter + ?Sized>(
    nodes: &mut [EdgeNode],
    fill_type: FillType,
    blitter: &mut B,
    start_y: i32,
    stop_y: i32,
    mut inverse: Option<InverseState>,
    right_clip: i32,
) {
    let mut curr_y = start_y;
    let winding_mask: i32 = if fill_type.is_even_odd() { 1 } else { -1 };

    loop {
        let mut w = 0i32;
        let mut left = 0i32;
        let mut curr_e = nodes[HEAD_IDX].next;
        let mut prev_x = nodes[HEAD_IDX].edge.base().x;

        if let Some(ref mut inv) = inverse {
            inv.pre();
        }

        while nodes[curr_e].edge.base().first_y <= curr_y {
            let x = fixed_round_to_int(nodes[curr_e].edge.base().x);

            if (w & winding_mask) == 0 {
                left = x;
            }

            w += nodes[curr_e].edge.base().winding as i32;

            if (w & winding_mask) == 0 {
                let width = x - left;
                debug_assert!(width >= 0);
                if width > 0 {
                    if let Some(ref mut inv) = inverse {
                        inv.blit_h(blitter, left, curr_y, width);
                    } else {
                        blitter.blit_h(left, curr_y, width);
                    }
                }
            }

            let next = nodes[curr_e].next;
            if nodes[curr_e].edge.base().last_y == curr_y {
                let advanced = if nodes[curr_e].edge.has_next_segment() {
                    nodes[curr_e].edge.next_segment()
                } else {
                    false
                };
                if advanced {
                    let new_x = nodes[curr_e].edge.base().x;
                    if new_x < prev_x {
                        backward_insert_edge_based_on_x(nodes, curr_e);
                    } else {
                        prev_x = new_x;
                    }
                } else {
                    remove_edge(nodes, curr_e);
                }
            } else {
                let new_x = nodes[curr_e]
                    .edge
                    .base()
                    .x
                    .wrapping_add(nodes[curr_e].edge.base().dx_dy);
                nodes[curr_e].edge.base_mut().x = new_x;
                if new_x < prev_x {
                    backward_insert_edge_based_on_x(nodes, curr_e);
                } else {
                    prev_x = new_x;
                }
            }
            curr_e = next;
        }

        if (w & winding_mask) != 0 {
            let width = right_clip - left;
            if width > 0 {
                if let Some(ref mut inv) = inverse {
                    inv.blit_h(blitter, left, curr_y, width);
                } else {
                    blitter.blit_h(left, curr_y, width);
                }
            }
        }

        if let Some(ref mut inv) = inverse {
            inv.post(blitter, curr_y);
        }

        curr_y += 1;
        if curr_y >= stop_y {
            break;
        }
        insert_new_edges(nodes, curr_e, curr_y);
    }
}

#[inline]
fn update_edge(edge: &mut BasicEdge, last_y: i32) -> bool {
    debug_assert!(edge.base().last_y >= last_y);
    if last_y != edge.base().last_y {
        return true;
    }
    if !edge.has_next_segment() {
        return false;
    }
    edge.next_segment()
}

fn walk_simple_edges<B: Blitter + ?Sized>(
    nodes: &mut [EdgeNode],
    blitter: &mut B,
    start_y: i32,
    stop_y: i32,
) {
    let mut left_e = nodes[HEAD_IDX].next;
    let mut rite_e = nodes[left_e].next;
    let mut curr_e = nodes[rite_e].next;

    let mut local_top = nodes[left_e]
        .edge
        .base()
        .first_y
        .max(nodes[rite_e].edge.base().first_y);
    if local_top < start_y {
        return;
    }

    while local_top < stop_y {
        let local_bot = nodes[left_e]
            .edge
            .base()
            .last_y
            .min(nodes[rite_e].edge.base().last_y)
            .min(stop_y - 1);
        if local_top > local_bot {
            return;
        }

        let mut left = nodes[left_e].edge.base().x;
        let d_left = nodes[left_e].edge.base().dx_dy;
        let mut rite = nodes[rite_e].edge.base().x;
        let d_rite = nodes[rite_e].edge.base().dx_dy;
        let mut count = local_bot - local_top;
        if count < 0 {
            return;
        }

        if d_left == 0 && d_rite == 0 {
            let mut l = fixed_round_to_int(left);
            let mut r = fixed_round_to_int(rite);
            if l > r {
                core::mem::swap(&mut l, &mut r);
            }
            if l < r {
                count += 1;
                blitter.blit_rect(l, local_top, r - l, count);
            }
            local_top = local_bot + 1;
        } else {
            loop {
                let mut l = fixed_round_to_int(left);
                let mut r = fixed_round_to_int(rite);
                if l > r {
                    core::mem::swap(&mut l, &mut r);
                }
                if l < r {
                    blitter.blit_h(l, local_top, r - l);
                }
                left = left.wrapping_add(d_left);
                rite = rite.wrapping_add(d_rite);
                local_top += 1;
                count -= 1;
                if count < 0 {
                    break;
                }
            }
        }

        nodes[left_e].edge.base_mut().x = left;
        nodes[rite_e].edge.base_mut().x = rite;

        if !update_edge(&mut nodes[left_e].edge, local_bot) {
            if nodes[curr_e].edge.base().first_y >= stop_y {
                return;
            }
            left_e = curr_e;
            curr_e = nodes[curr_e].next;
            if nodes[left_e].edge.base().first_y != local_top {
                return;
            }
        }
        if !update_edge(&mut nodes[rite_e].edge, local_bot) {
            if nodes[curr_e].edge.base().first_y >= stop_y {
                return;
            }
            rite_e = curr_e;
            curr_e = nodes[curr_e].next;
            if nodes[rite_e].edge.base().first_y != local_top {
                return;
            }
        }
    }
}

fn build_sorted_edge_nodes(mut edges: Vec<BasicEdge>) -> Vec<EdgeNode> {
    sk_tqsort(&mut edges, &|a, b| {
        let ba = a.base();
        let bb = b.base();
        if ba.first_y != bb.first_y {
            ba.first_y < bb.first_y
        } else {
            ba.x < bb.x
        }
    });

    let count = edges.len();
    let mut nodes = Vec::with_capacity(count + 2);

    let mut head_base = EdgeBase::default();
    head_base.first_y = EDGE_HEAD_Y;
    head_base.x = SK_MIN_S32;
    nodes.push(EdgeNode {
        edge: BasicEdge::Line(head_base),
        prev: NONE,
        next: 2,
    });

    let mut tail_base = EdgeBase::default();
    tail_base.first_y = EDGE_TAIL_Y;
    tail_base.x = SK_MAX_S32;
    nodes.push(EdgeNode {
        edge: BasicEdge::Line(tail_base),
        prev: 2 + count - 1,
        next: NONE,
    });

    for (i, e) in edges.into_iter().enumerate() {
        let idx = 2 + i;
        let prev = if i == 0 { HEAD_IDX } else { idx - 1 };
        let next = if i + 1 == count { TAIL_IDX } else { idx + 1 };
        nodes.push(EdgeNode {
            edge: e,
            prev,
            next,
        });
    }

    nodes
}

/// Matches `sk_fill_path` in `SkScan_Path.cpp`.
pub fn fill_path_clipped<B: Blitter + ?Sized>(
    raw: &PathRaw<'_>,
    clip_rect: IRect,
    blitter: &mut B,
    mut start_y: i32,
    mut stop_y: i32,
    path_contained_in_clip: bool,
) {
    let mut builder = BasicEdgeBuilder::new();
    let count = builder.build_edges(
        raw,
        if path_contained_in_clip {
            None
        } else {
            Some(clip_rect)
        },
    );

    if count == 0 {
        if raw.fill_type.is_inverse() {
            let mut rect = clip_rect;
            if rect.top < start_y {
                rect.top = start_y;
            }
            if rect.bottom > stop_y {
                rect.bottom = stop_y;
            }
            if !rect.is_empty() {
                blitter.blit_rect(rect.left, rect.top, rect.width(), rect.height());
            }
        }
        return;
    }

    let mut nodes = build_sorted_edge_nodes(builder.edges);

    if !path_contained_in_clip && start_y < clip_rect.top {
        start_y = clip_rect.top;
    }
    if !path_contained_in_clip && stop_y > clip_rect.bottom {
        stop_y = clip_rect.bottom;
    }

    let is_inverse = raw.fill_type.is_inverse();
    if raw.convexity.is_convex() && !is_inverse && count >= 2 {
        walk_simple_edges(&mut nodes, blitter, start_y, stop_y);
    } else {
        let inv = if is_inverse {
            Some(InverseState::new(clip_rect))
        } else {
            None
        };
        walk_edges(
            &mut nodes,
            raw.fill_type,
            blitter,
            start_y,
            stop_y,
            inv,
            clip_rect.right,
        );
    }
}

/// Matches `sk_fill_triangle` in `SkScan_Path.cpp`.
pub fn fill_triangle_clipped<B: Blitter + ?Sized>(
    pts: &[Point; 3],
    clip_rect: Option<IRect>,
    blitter: &mut B,
    ir: IRect,
) {
    let mut edges = Vec::with_capacity(3);
    let pairs = [(pts[0], pts[1]), (pts[1], pts[2]), (pts[2], pts[0])];
    for (p0, p1) in pairs {
        let mut e = EdgeBase::default();
        if e.set_line(p0, p1, clip_rect) {
            edges.push(BasicEdge::Line(e));
        }
    }
    if edges.len() < 2 {
        return;
    }

    let mut nodes = build_sorted_edge_nodes(edges);

    let mut stop_y = ir.bottom;
    if let Some(cr) = clip_rect {
        if stop_y > cr.bottom {
            stop_y = cr.bottom;
        }
    }
    let mut start_y = ir.top;
    if let Some(cr) = clip_rect {
        if start_y < cr.top {
            start_y = cr.top;
        }
    }
    walk_simple_edges(&mut nodes, blitter, start_y, stop_y);
}
