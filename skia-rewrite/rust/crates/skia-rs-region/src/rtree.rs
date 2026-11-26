#![forbid(unsafe_code)]

//! Safe Rust port of `src/core/SkRTree.cpp` and `src/core/SkBBHFactory.cpp`.
//!
//! Bottom-up bulk-loaded balanced n-ary R-Tree matching Skia's exact node
//! partitioning (`kMinChildren = 6`, `kMaxChildren = 11`) and search order.

use skia_rs_geometry::Rect;

pub const MIN_CHILDREN: usize = 6;
pub const MAX_CHILDREN: usize = 11;

/// A single child entry inside an `RTreeNode`.
///
/// For leaf nodes (`level == 0`), `index` is the original op index in the input
/// slice. For internal nodes (`level > 0`), `index` is an index into `RTree::nodes`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct RTreeBranch {
    pub index: u32,
    pub bounds: Rect,
}

/// An R-Tree node with up to `MAX_CHILDREN` children.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RTreeNode {
    pub num_children: u16,
    pub level: u16,
    pub children: [RTreeBranch; MAX_CHILDREN],
}

impl Default for RTreeNode {
    fn default() -> Self {
        Self {
            num_children: 0,
            level: 0,
            children: [RTreeBranch::default(); MAX_CHILDREN],
        }
    }
}

/// Safe Rust R-Tree matching `SkRTree`.
#[derive(Clone, Debug, Default)]
pub struct RTree {
    pub count: usize,
    pub root_bounds: Rect,
    pub root_node: u32,
    pub nodes: Vec<RTreeNode>,
}

/// Matches `SkRTree::CountNodes(int branches)`.
///
/// Counts exactly how many nodes `bulk_load` will allocate for `branches` input items.
pub fn count_nodes(branches: usize) -> usize {
    if branches <= 1 {
        return branches;
    }
    let mut remainder = branches % MAX_CHILDREN;
    if remainder > 0 {
        if remainder >= MIN_CHILDREN {
            remainder = 0;
        } else {
            remainder = MIN_CHILDREN - remainder;
        }
    }
    let mut current_branch = 0;
    let mut nodes = 0;
    while current_branch < branches {
        let mut increment_by = MAX_CHILDREN;
        if remainder != 0 {
            if remainder <= MAX_CHILDREN - MIN_CHILDREN {
                increment_by -= remainder;
                remainder = 0;
            } else {
                increment_by = MIN_CHILDREN;
                remainder -= MAX_CHILDREN - MIN_CHILDREN;
            }
        }
        nodes += 1;
        current_branch += 1;
        let mut k = 1;
        while k < increment_by && current_branch < branches {
            current_branch += 1;
            k += 1;
        }
    }
    nodes + count_nodes(nodes)
}

/// Zero-allocation recursive search over any R-Tree node representation.
///
/// `read_node` receives a node index and invokes the provided callback or returns
/// `(level, num_children, children_array)`.
pub fn search_with<F, H>(
    root_bounds: Rect,
    root_node: usize,
    count: usize,
    query: Rect,
    read_node: &F,
    on_hit: &mut H,
) where
    F: Fn(usize) -> (u16, u16, [RTreeBranch; MAX_CHILDREN]),
    H: FnMut(i32),
{
    if count == 0 || !Rect::intersects(root_bounds, query) {
        return;
    }
    search_node_with(root_node, query, read_node, on_hit);
}

fn search_node_with<F, H>(node_idx: usize, query: Rect, read_node: &F, on_hit: &mut H)
where
    F: Fn(usize) -> (u16, u16, [RTreeBranch; MAX_CHILDREN]),
    H: FnMut(i32),
{
    let (level, num_children, children) = read_node(node_idx);
    let n = (num_children as usize).min(MAX_CHILDREN);
    for child in &children[..n] {
        if Rect::intersects(child.bounds, query) {
            if level == 0 {
                on_hit(child.index as i32);
            } else {
                search_node_with(child.index as usize, query, read_node, on_hit);
            }
        }
    }
}

impl RTree {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bulk-loads bounding rectangles into a new `RTree`, skipping empty rects.
    /// Matches `SkRTree::insert(const SkRect[], int N)`.
    pub fn build(bounds_array: &[Rect]) -> Self {
        let mut branches = Vec::with_capacity(bounds_array.len());
        for (i, &bounds) in bounds_array.iter().enumerate() {
            if bounds.is_empty() {
                continue;
            }
            branches.push(RTreeBranch {
                index: i as u32,
                bounds,
            });
        }

        let count = branches.len();
        if count == 0 {
            return Self::default();
        }

        if count == 1 {
            let mut node = RTreeNode {
                num_children: 1,
                level: 0,
                children: [RTreeBranch::default(); MAX_CHILDREN],
            };
            node.children[0] = branches[0];
            return Self {
                count: 1,
                root_bounds: branches[0].bounds,
                root_node: 0,
                nodes: vec![node],
            };
        }

        let mut tree = Self {
            count,
            root_bounds: Rect::make_empty(),
            root_node: 0,
            nodes: Vec::with_capacity(count_nodes(count)),
        };
        let root = tree.bulk_load(&mut branches, 0);
        tree.root_bounds = root.bounds;
        tree.root_node = root.index;
        tree
    }

    fn allocate_node_at_level(&mut self, level: u16) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(RTreeNode {
            num_children: 0,
            level,
            children: [RTreeBranch::default(); MAX_CHILDREN],
        });
        idx
    }

    fn bulk_load(&mut self, branches: &mut Vec<RTreeBranch>, level: u16) -> RTreeBranch {
        if branches.len() == 1 {
            return branches[0];
        }

        let total = branches.len();
        let mut remainder = total % MAX_CHILDREN;
        let mut new_branches = 0;

        if remainder > 0 {
            if remainder >= MIN_CHILDREN {
                remainder = 0;
            } else {
                remainder = MIN_CHILDREN - remainder;
            }
        }

        let mut current_branch = 0;
        while current_branch < total {
            let mut increment_by = MAX_CHILDREN;
            if remainder != 0 {
                if remainder <= MAX_CHILDREN - MIN_CHILDREN {
                    increment_by -= remainder;
                    remainder = 0;
                } else {
                    increment_by = MIN_CHILDREN;
                    remainder -= MAX_CHILDREN - MIN_CHILDREN;
                }
            }
            let node_idx = self.allocate_node_at_level(level);
            let first = branches[current_branch];
            self.nodes[node_idx].num_children = 1;
            self.nodes[node_idx].children[0] = first;
            let mut b = RTreeBranch {
                index: node_idx as u32,
                bounds: first.bounds,
            };
            current_branch += 1;

            let mut k = 1;
            while k < increment_by && current_branch < total {
                let item = branches[current_branch];
                b.bounds.join(item.bounds);
                self.nodes[node_idx].children[k] = item;
                self.nodes[node_idx].num_children += 1;
                current_branch += 1;
                k += 1;
            }
            branches[new_branches] = b;
            new_branches += 1;
        }
        branches.truncate(new_branches);
        self.bulk_load(branches, level + 1)
    }

    /// Returns the depth of the tree (`0` if empty).
    #[inline]
    pub fn depth(&self) -> usize {
        if self.count == 0 {
            0
        } else {
            self.nodes[self.root_node as usize].level as usize + 1
        }
    }

    /// Returns the number of non-empty inserted bounding boxes.
    #[inline]
    pub fn count(&self) -> usize {
        self.count
    }

    /// Populates `results` with all op indices whose bounds intersect `query`.
    pub fn search(&self, query: Rect, results: &mut Vec<i32>) {
        search_with(
            self.root_bounds,
            self.root_node as usize,
            self.count,
            query,
            &|idx| {
                let n = &self.nodes[idx];
                (n.level, n.num_children, n.children)
            },
            &mut |op_idx| results.push(op_idx),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_single() {
        let t0 = RTree::build(&[]);
        assert_eq!(t0.count(), 0);
        assert_eq!(t0.depth(), 0);
        let mut res = Vec::new();
        t0.search(Rect::make_ltrb(0.0, 0.0, 100.0, 100.0), &mut res);
        assert!(res.is_empty());

        let t1 = RTree::build(&[Rect::make_empty(), Rect::make_ltrb(10.0, 20.0, 30.0, 40.0)]);
        assert_eq!(t1.count(), 1);
        assert_eq!(t1.depth(), 1);
        t1.search(Rect::make_ltrb(15.0, 25.0, 16.0, 26.0), &mut res);
        assert_eq!(res, vec![1]);
    }

    #[test]
    fn count_nodes_matches_allocated() {
        for n in 1..=200 {
            let rects: Vec<Rect> = (0..n)
                .map(|i| Rect::make_ltrb(i as f32, 0.0, (i + 1) as f32, 10.0))
                .collect();
            let tree = RTree::build(&rects);
            assert_eq!(tree.count(), n);
            // SkRTree::CountNodes returns 1 for the base case `branches == 1`, whereas
            // `bulkLoad` stops without allocating at `branches == 1`, so `CountNodes(n)`
            // equals `nodes.len()` for n == 1 and `nodes.len() + 1` for n >= 2.
            let expected_count = if n == 1 {
                tree.nodes.len()
            } else {
                tree.nodes.len() + 1
            };
            assert_eq!(count_nodes(n), expected_count);
            if n == 1 {
                assert_eq!(tree.depth(), 1);
            } else if n <= MAX_CHILDREN {
                assert_eq!(tree.depth(), 1);
            } else if n <= MAX_CHILDREN * MAX_CHILDREN {
                assert_eq!(tree.depth(), 2);
            } else {
                assert_eq!(tree.depth(), 3);
            }
        }
    }

    #[test]
    fn search_matches_linear_scan() {
        let mut rects = Vec::new();
        for i in 0..150 {
            let x = ((i * 37) % 100) as f32;
            let y = ((i * 53) % 100) as f32;
            if i % 13 == 0 {
                rects.push(Rect::make_empty());
            } else {
                rects.push(Rect::make_ltrb(x, y, x + 15.0, y + 12.0));
            }
        }
        let tree = RTree::build(&rects);

        let queries = [
            Rect::make_ltrb(0.0, 0.0, 200.0, 200.0),
            Rect::make_ltrb(25.0, 25.0, 50.0, 50.0),
            Rect::make_ltrb(-10.0, -10.0, -5.0, -5.0),
            Rect::make_ltrb(90.0, 90.0, 105.0, 105.0),
            Rect::make_empty(),
        ];

        for q in queries {
            let mut got = Vec::new();
            tree.search(q, &mut got);
            let expected: Vec<i32> = rects
                .iter()
                .enumerate()
                .filter(|(_, &r)| !r.is_empty() && Rect::intersects(r, q))
                .map(|(i, _)| i as i32)
                .collect();
            assert_eq!(got, expected);
        }
    }
}
