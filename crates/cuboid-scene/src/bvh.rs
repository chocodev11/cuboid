//! Binned-SAH bounding volume hierarchy over world bounds (spec: arch 13.7).
//!
//! The tree stores indices into the slice of bounds it was built from. A transform change calls
//! [`Bvh::refit`], which keeps the shape and recomputes the bounds in one linear pass; a structural
//! change builds again. [`Bvh::degraded`] tells when refits have stretched the tree enough to
//! be worth a rebuild.

use glam::Vec3;

use crate::geom::{Aabb, Frustum, clamped_index, inv_dir, to_u32};

/// Items below which a node becomes a leaf without looking for a split.
const LEAF_MAX: usize = 4;

/// Largest leaf made when the items cannot be told apart by position.
const LEAF_FALLBACK: usize = 16;

/// Bins of the surface-area-heuristic sweep.
const BINS: usize = 16;

/// A node: its bounds and either a leaf range (`count > 0`, `first` indexes [`Bvh::order`]) or the
/// index of the first of two adjacent children (`count == 0`).
#[derive(Copy, Clone, Debug)]
struct Node {
    bounds: Aabb,
    first: u32,
    count: u32,
}

/// A BVH over world bounds, refit on transform changes and rebuilt on structural changes.
///
/// spec: arch 13.7
#[derive(Debug, Default)]
pub struct Bvh {
    nodes: Vec<Node>,
    order: Vec<u32>,
    build_area: f32,
}

/// Where the SAH sweep splits: the bin boundary and the cost.
struct Split {
    bin: usize,
    cost: f32,
}

impl Bvh {
    /// Builds the tree over `bounds`; item `i` is `bounds[i]`. Items with an empty or invalid box
    /// stay in the tree but are never hit.
    pub fn build(bounds: &[Aabb]) -> Bvh {
        let n = bounds.len();
        if n == 0 {
            return Bvh::default();
        }
        let centers: Vec<Vec3> = bounds
            .iter()
            .map(|b| if b.is_valid() { b.center() } else { Vec3::ZERO })
            .collect();
        let mut order: Vec<u32> = (0..n).map(to_u32).collect();
        let mut nodes = Vec::with_capacity(n / 2 + 2);
        nodes.push(Node {
            bounds: Aabb::EMPTY,
            first: 0,
            count: 0,
        });
        let mut stack = vec![(0usize, 0usize, n)];
        while let Some((node, start, end)) = stack.pop() {
            let range = &order[start..end];
            let (mut bb, mut cb) = (Aabb::EMPTY, Aabb::EMPTY);
            for &i in range {
                bb = bb.union(bounds[i as usize].valid_or_empty());
                cb = cb.with_point(centers[i as usize]);
            }
            nodes[node].bounds = bb;
            let count = end - start;
            if count <= LEAF_MAX {
                nodes[node].first = to_u32(start);
                nodes[node].count = to_u32(count);
                continue;
            }
            let extent = cb.max - cb.min;
            let axis = if extent.x >= extent.y && extent.x >= extent.z {
                0
            } else if extent.y >= extent.z {
                1
            } else {
                2
            };
            let mid = if extent[axis] > 1e-9 {
                let split = sah_split(bounds, &centers, &order[start..end], axis, &cb, bb.area());
                match split {
                    Some(Split { bin, .. }) => {
                        let m = partition(&mut order[start..end], |i| {
                            bin_of(centers[i as usize][axis], cb.min[axis], extent[axis]) <= bin
                        });
                        if m == 0 || m == count {
                            median(&mut order[start..end], &centers, axis)
                        } else {
                            m
                        }
                    }
                    None if count <= LEAF_FALLBACK => {
                        nodes[node].first = to_u32(start);
                        nodes[node].count = to_u32(count);
                        continue;
                    }
                    None => median(&mut order[start..end], &centers, axis),
                }
            } else if count <= LEAF_FALLBACK {
                nodes[node].first = to_u32(start);
                nodes[node].count = to_u32(count);
                continue;
            } else {
                count / 2
            };
            let left = nodes.len();
            nodes.push(Node {
                bounds: Aabb::EMPTY,
                first: 0,
                count: 0,
            });
            nodes.push(Node {
                bounds: Aabb::EMPTY,
                first: 0,
                count: 0,
            });
            nodes[node].first = to_u32(left);
            stack.push((left, start, start + mid));
            stack.push((left + 1, start + mid, end));
        }
        let build_area = nodes[0].bounds.area();
        Bvh {
            nodes,
            order,
            build_area,
        }
    }

    /// The number of items the tree was built over.
    pub fn len(&self) -> usize {
        self.order.len()
    }

    /// Whether the tree has no items.
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// The box that holds every item, `None` for an empty tree.
    pub fn bounds(&self) -> Option<Aabb> {
        self.nodes.first().map(|n| n.bounds)
    }

    /// Recomputes the node bounds for new item bounds, keeping the shape. Returns `false`
    /// without changing anything when the number of items differs; build again then.
    pub fn refit(&mut self, bounds: &[Aabb]) -> bool {
        if bounds.len() != self.order.len() {
            return false;
        }
        // Children sit after their parent, so one backward pass sees them first.
        for i in (0..self.nodes.len()).rev() {
            let node = self.nodes[i];
            let merged = if node.count > 0 {
                let (first, count) = (node.first as usize, node.count as usize);
                self.order[first..first + count]
                    .iter()
                    .fold(Aabb::EMPTY, |acc, &item| {
                        acc.union(bounds[item as usize].valid_or_empty())
                    })
            } else {
                let child = node.first as usize;
                self.nodes[child].bounds.union(self.nodes[child + 1].bounds)
            };
            self.nodes[i].bounds = merged;
        }
        true
    }

    /// Whether refits stretched the tree to more than twice the surface it was built with.
    pub fn degraded(&self) -> bool {
        self.nodes
            .first()
            .is_some_and(|root| root.bounds.area() > self.build_area * 2.0 + 1e-3)
    }

    /// Walks the tree front to back along a ray and returns the item whose `test` reports the
    /// smallest parameter, with that parameter. `test(item, limit)` returns the ray parameter of
    /// the hit when it is at most `limit`. `dir` need not be a unit vector; the parameters are in
    /// its units.
    pub fn nearest(
        &self,
        origin: Vec3,
        dir: Vec3,
        max_t: f32,
        mut test: impl FnMut(u32, f32) -> Option<f32>,
    ) -> Option<(u32, f32)> {
        let root = self.nodes.first()?;
        let inv = inv_dir(dir);
        root.bounds.ray_entry(origin, inv, max_t)?;
        let mut best: Option<(u32, f32)> = None;
        let mut limit = max_t;
        let mut stack: Vec<u32> = Vec::with_capacity(64);
        stack.push(0);
        while let Some(index) = stack.pop() {
            let node = self.nodes[index as usize];
            if node.count > 0 {
                let (first, count) = (node.first as usize, node.count as usize);
                for &item in &self.order[first..first + count] {
                    if let Some(t) = test(item, limit)
                        && t <= limit
                        && best.is_none_or(|(_, b)| t < b)
                    {
                        best = Some((item, t));
                        limit = t;
                    }
                }
                continue;
            }
            let (l, r) = (node.first, node.first + 1);
            let el = self.nodes[l as usize].bounds.ray_entry(origin, inv, limit);
            let er = self.nodes[r as usize].bounds.ray_entry(origin, inv, limit);
            match (el, er) {
                (Some(a), Some(b)) if a <= b => stack.extend([r, l]),
                (Some(_), Some(_)) => stack.extend([l, r]),
                (Some(_), None) => stack.push(l),
                (None, Some(_)) => stack.push(r),
                (None, None) => {}
            }
        }
        best
    }

    /// Calls `visit` for every item whose leaf box may intersect the frustum.
    pub fn query_frustum(&self, frustum: &Frustum, mut visit: impl FnMut(u32)) {
        self.query(|b| frustum.intersects_aabb(b), &mut visit);
    }

    /// Calls `visit` for every item whose leaf box overlaps `area`.
    pub fn query_aabb(&self, area: &Aabb, mut visit: impl FnMut(u32)) {
        self.query(|b| b.is_valid() && b.overlaps(area), &mut visit);
    }

    fn query(&self, keep: impl Fn(&Aabb) -> bool, visit: &mut impl FnMut(u32)) {
        if self.nodes.is_empty() {
            return;
        }
        let mut stack: Vec<u32> = vec![0];
        while let Some(index) = stack.pop() {
            let node = self.nodes[index as usize];
            if !keep(&node.bounds) {
                continue;
            }
            if node.count > 0 {
                let (first, count) = (node.first as usize, node.count as usize);
                self.order[first..first + count]
                    .iter()
                    .for_each(|&i| visit(i));
            } else {
                stack.extend([node.first, node.first + 1]);
            }
        }
    }
}

/// The bin of a centroid coordinate along the split axis.
fn bin_of(c: f32, lo: f32, extent: f32) -> usize {
    clamped_index((c - lo) / extent * BINS as f32, BINS)
}

/// Chooses the cheapest bin boundary of the SAH sweep, `None` when keeping the node as a leaf
/// is cheaper.
fn sah_split(
    bounds: &[Aabb],
    centers: &[Vec3],
    items: &[u32],
    axis: usize,
    cb: &Aabb,
    parent_area: f32,
) -> Option<Split> {
    let extent = cb.max[axis] - cb.min[axis];
    let mut bins = [(Aabb::EMPTY, 0usize); BINS];
    for &i in items {
        let bin = bin_of(centers[i as usize][axis], cb.min[axis], extent);
        bins[bin].0 = bins[bin].0.union(bounds[i as usize].valid_or_empty());
        bins[bin].1 += 1;
    }
    let mut right_cost = [0.0_f32; BINS];
    let (mut acc, mut n) = (Aabb::EMPTY, 0usize);
    for i in (1..BINS).rev() {
        acc = acc.union(bins[i].0);
        n += bins[i].1;
        right_cost[i - 1] = acc.area() * n as f32;
    }
    let (mut acc, mut n) = (Aabb::EMPTY, 0usize);
    let mut best: Option<Split> = None;
    for (i, bin) in bins.iter().take(BINS - 1).enumerate() {
        acc = acc.union(bin.0);
        n += bin.1;
        if n == 0 || n == items.len() {
            continue;
        }
        let cost = acc.area() * n as f32 + right_cost[i];
        if best.as_ref().is_none_or(|b| cost < b.cost) {
            best = Some(Split { bin: i, cost });
        }
    }
    // A leaf costs the parent area per item; a split must beat it clearly for big nodes.
    let leaf_cost = parent_area * items.len() as f32;
    best.filter(|b| items.len() > LEAF_FALLBACK || b.cost < leaf_cost)
}

/// Moves the items for which `left` holds to the front; returns how many there are.
fn partition(items: &mut [u32], left: impl Fn(u32) -> bool) -> usize {
    let mut mid = 0;
    for i in 0..items.len() {
        if left(items[i]) {
            items.swap(i, mid);
            mid += 1;
        }
    }
    mid
}

/// Splits the items at the median centroid on `axis`; returns the size of the left half.
fn median(items: &mut [u32], centers: &[Vec3], axis: usize) -> usize {
    let mid = items.len() / 2;
    items.select_nth_unstable_by(mid, |&a, &b| {
        centers[a as usize][axis].total_cmp(&centers[b as usize][axis])
    });
    mid
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn boxes(seed: &[(f32, f32, f32, f32)]) -> Vec<Aabb> {
        seed.iter()
            .map(|&(x, y, z, s)| {
                let c = Vec3::new(x, y, z);
                Aabb::from_corners(c - Vec3::splat(s), c + Vec3::splat(s))
            })
            .collect()
    }

    fn brute(bounds: &[Aabb], origin: Vec3, dir: Vec3, max_t: f32) -> Option<f32> {
        let inv = inv_dir(dir);
        bounds
            .iter()
            .filter_map(|b| b.ray_entry(origin, inv, max_t))
            .min_by(f32::total_cmp)
    }

    fn via_bvh(bvh: &Bvh, bounds: &[Aabb], origin: Vec3, dir: Vec3, max_t: f32) -> Option<f32> {
        let inv = inv_dir(dir);
        bvh.nearest(origin, dir, max_t, |i, limit| {
            bounds[i as usize].ray_entry(origin, inv, limit)
        })
        .map(|(_, t)| t)
    }

    #[test]
    fn an_empty_tree_answers_nothing() {
        let bvh = Bvh::build(&[]);
        assert!(bvh.is_empty() && bvh.bounds().is_none());
        assert!(
            bvh.nearest(Vec3::ZERO, Vec3::X, 10.0, |_, _| Some(1.0))
                .is_none()
        );
        assert!(!Bvh::default().degraded());
    }

    #[test]
    fn identical_items_still_split_into_small_leaves() {
        let same = vec![Aabb::from_corners(Vec3::ZERO, Vec3::ONE); 1000];
        let bvh = Bvh::build(&same);
        let hit = via_bvh(&bvh, &same, Vec3::new(-2.0, 0.5, 0.5), Vec3::X, 10.0);
        assert!(hit.is_some_and(|t| (t - 2.0).abs() < 1e-5));
    }

    #[test]
    fn invalid_boxes_are_kept_but_never_hit() {
        let mut list = boxes(&[(0.0, 0.0, 0.0, 1.0), (5.0, 0.0, 0.0, 1.0)]);
        list.push(Aabb::EMPTY);
        list.push(Aabb {
            min: Vec3::NAN,
            max: Vec3::NAN,
        });
        let bvh = Bvh::build(&list);
        assert_eq!(bvh.len(), 4);
        let t = via_bvh(&bvh, &list, Vec3::new(-5.0, 0.0, 0.0), Vec3::X, 100.0);
        assert!(t.is_some_and(|t| (t - 4.0).abs() < 1e-5));
    }

    #[test]
    fn refit_follows_moved_items_and_rejects_a_changed_count() {
        let mut list = boxes(&[
            (0.0, 0.0, 0.0, 1.0),
            (10.0, 0.0, 0.0, 1.0),
            (20.0, 0.0, 0.0, 1.0),
        ]);
        let mut bvh = Bvh::build(&list);
        list[0] = Aabb::from_corners(Vec3::new(30.0, -1.0, -1.0), Vec3::new(32.0, 1.0, 1.0));
        assert!(bvh.refit(&list));
        let hit = via_bvh(&bvh, &list, Vec3::new(40.0, 0.0, 0.0), Vec3::NEG_X, 100.0);
        assert!(hit.is_some_and(|t| (t - 8.0).abs() < 1e-5));
        list.pop();
        assert!(!bvh.refit(&list));
    }

    #[test]
    fn refits_that_stretch_the_tree_are_reported() {
        let mut list: Vec<Aabb> = (0..64)
            .map(|i| {
                let c = Vec3::new(i as f32, 0.0, 0.0);
                Aabb::from_corners(c, c + Vec3::ONE)
            })
            .collect();
        let mut bvh = Bvh::build(&list);
        assert!(!bvh.degraded());
        for (i, b) in list.iter_mut().enumerate() {
            let c = Vec3::new(0.0, (i * 50) as f32, 0.0);
            *b = Aabb::from_corners(c, c + Vec3::ONE);
        }
        bvh.refit(&list);
        assert!(bvh.degraded());
    }

    #[test]
    fn frustum_and_box_queries_find_the_overlapping_items() {
        // Items spread along X; the queries return the items of the leaves that overlap.
        let seed: Vec<_> = (0..200).map(|i| (i as f32 * 10.0, 0.0, 0.0, 1.0)).collect();
        let list = boxes(&seed);
        let bvh = Bvh::build(&list);
        let mut found = Vec::new();
        bvh.query_aabb(
            &Aabb::from_corners(Vec3::new(-2.0, -2.0, -2.0), Vec3::new(3.0, 3.0, 3.0)),
            |i| found.push(i),
        );
        assert!(found.contains(&0) && found.len() < 20, "{found:?}");
        let cam = crate::Camera::default_for(crate::ViewId::Front);
        let frustum = cam.frustum(1.0, glam::Vec2::splat(-1.0), glam::Vec2::ONE);
        let mut seen = Vec::new();
        bvh.query_frustum(&frustum, |i| seen.push(i));
        assert!(seen.contains(&0) && seen.len() < 100, "{}", seen.len());
    }

    proptest! {
        #[test]
        fn the_nearest_hit_equals_the_brute_force_scan(
            seed in proptest::collection::vec((-50.0f32..50.0, -50.0..50.0f32, -50.0..50.0f32, 0.1..6.0f32), 1..200),
            origin in (-80.0f32..80.0, -80.0..80.0f32, -80.0..80.0f32),
            dir in (-1.0f32..1.0, -1.0..1.0f32, -1.0..1.0f32),
        ) {
            let list = boxes(&seed);
            let bvh = Bvh::build(&list);
            let dir = crate::geom::unit_or(Vec3::new(dir.0, dir.1, dir.2), Vec3::X);
            let origin = Vec3::new(origin.0, origin.1, origin.2);
            let expected = brute(&list, origin, dir, 300.0);
            let got = via_bvh(&bvh, &list, origin, dir, 300.0);
            match (expected, got) {
                (None, None) => {}
                (Some(a), Some(b)) => prop_assert!((a - b).abs() < 1e-4, "{a} vs {b}"),
                other => prop_assert!(false, "{other:?}"),
            }
        }

        #[test]
        fn a_refit_tree_answers_like_a_fresh_one(
            seed in proptest::collection::vec((-50.0f32..50.0, -50.0..50.0f32, -50.0..50.0f32, 0.1..6.0f32), 2..100),
            shift in (-30.0f32..30.0, -30.0..30.0f32, -30.0..30.0f32),
        ) {
            let mut list = boxes(&seed);
            let mut bvh = Bvh::build(&list);
            let delta = Vec3::new(shift.0, shift.1, shift.2);
            for b in list.iter_mut().step_by(2) {
                *b = Aabb { min: b.min + delta, max: b.max + delta };
            }
            prop_assert!(bvh.refit(&list));
            let fresh = Bvh::build(&list);
            for dir in [Vec3::X, Vec3::NEG_Y, Vec3::new(0.3, 0.5, -0.8).normalize()] {
                let origin = Vec3::new(-90.0, 5.0, 5.0);
                let a = via_bvh(&bvh, &list, origin, dir, 400.0);
                let b = via_bvh(&fresh, &list, origin, dir, 400.0);
                prop_assert_eq!(a.is_some(), b.is_some());
                if let (Some(a), Some(b)) = (a, b) {
                    prop_assert!((a - b).abs() < 1e-4);
                }
            }
        }
    }
}
