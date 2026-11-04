//! The broadphase tree: a dynamic AABB tree over fat boxes, written once for every dimension
//! through [`Bounds`].
//!
//! Every leaf holds the tight box of an object grown by a margin (the fat box) and a caller id; an
//! object that stays inside its fat box costs nothing to update, one that leaves it is taken out and
//! inserted again. Insertion descends by the surface-area heuristic and every change rebalances on
//! the way up with AVL rotations, so the two children of a node never differ in height by more than
//! one and the height stays below `floor(log2(n + 2)) * 1.44 + 1`.
//!
//! Storage is one vector of nodes allocated by [`Tree::new`] (two nodes per proxy, one leaf and one
//! internal node), linked by indices, never pointers; nothing allocates afterwards. Free nodes form
//! a stack (last released, first reused), so the same calls build the same tree on every run. No
//! step depends on randomness, a hash or an address; a tie in the insertion cost goes to the second
//! child. Queries walk the tree without a stack (through the parent links), so they cannot overflow
//! and allocate nothing.

use super::MAX_BODIES;
use super::bounds::Bounds;
use super::error::PhysicsError;
use super::hash::StateHasher;

/// The index that names no node.
const NIL: u32 = u32::MAX;
/// The height of a node that is on the free stack.
const FREE_HEIGHT: u32 = u32::MAX;

#[derive(Clone, Copy)]
struct Node<B> {
    bounds: B,
    parent: u32,
    child1: u32,
    child2: u32,
    height: u32,
    id: u32,
}

impl<B: Bounds> Node<B> {
    /// A node on the free stack; `next` is the node below it.
    fn free(next: u32) -> Self {
        Self {
            bounds: B::default(),
            parent: next,
            child1: NIL,
            child2: NIL,
            height: FREE_HEIGHT,
            id: NIL,
        }
    }

    /// Writes the box, the links, the height and the id.
    fn write_to(&self, hasher: &mut StateHasher) {
        self.bounds.write_bits(hasher);
        for value in [self.parent, self.child1, self.child2, self.height, self.id] {
            hasher.write_u32(value);
        }
    }

    /// A leaf holds an object; a node without children is a leaf (a free node reads as one, which
    /// keeps every walk finite on a tree that was damaged from outside).
    fn is_leaf(&self) -> bool {
        self.child1 == NIL
    }
}

/// Names one leaf of a [`Tree`]. Only the world's broadphase uses it today; make it crate private or
/// add a generation before another module starts to hold proxies.
/// The tree does not detect a proxy of another tree, nor one whose
/// leaf was freed and given to a later proxy: the owner (the world) keeps one per object and drops
/// it on removal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProxyId(u32);

impl ProxyId {
    /// The node index of the leaf.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

/// A dynamic AABB tree with a fixed capacity.
pub struct Tree<B: Bounds> {
    nodes: Vec<Node<B>>,
    root: u32,
    free: u32,
    proxies: u32,
    capacity: u32,
    margin: f32,
}

impl<B: Bounds> Tree<B> {
    /// Builds a tree for at most `max_proxies` leaves whose boxes are grown by `margin`. This is the
    /// only call that allocates.
    ///
    /// # Errors
    /// [`PhysicsError::CapacityTooLarge`] above [`MAX_BODIES`]; [`PhysicsError::InvalidMargin`] when
    /// the margin is negative or not finite.
    pub fn new(max_proxies: u32, margin: f32) -> Result<Self, PhysicsError> {
        if max_proxies > MAX_BODIES {
            return Err(PhysicsError::CapacityTooLarge { max: MAX_BODIES });
        }
        if !margin.is_finite() || margin < 0.0 {
            return Err(PhysicsError::InvalidMargin);
        }
        let count = (2 * max_proxies).saturating_sub(1);
        let nodes: Vec<Node<B>> = (0..count)
            .map(|i| Node::free(if i + 1 < count { i + 1 } else { NIL }))
            .collect();
        Ok(Self {
            free: if count > 0 { 0 } else { NIL },
            nodes,
            root: NIL,
            proxies: 0,
            capacity: max_proxies,
            margin,
        })
    }

    /// The most proxies the tree can hold.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity as usize
    }

    /// The number of proxies.
    #[must_use]
    pub fn len(&self) -> usize {
        self.proxies as usize
    }

    /// True when the tree holds no proxy.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.proxies == 0
    }

    /// The margin the stored boxes are grown by.
    #[must_use]
    pub fn margin(&self) -> f32 {
        self.margin
    }

    /// The height of the tree: zero for an empty tree and for one leaf.
    #[must_use]
    pub fn height(&self) -> u32 {
        if self.root == NIL {
            0
        } else {
            self.node(self.root).height
        }
    }

    /// Adds a proxy for an object with the tight box `tight` and the caller's `id`.
    ///
    /// # Errors
    /// [`PhysicsError::NotFinite`] when `tight` or its fat box is not valid;
    /// [`PhysicsError::CapacityExhausted`] when the tree is full. Nothing changes on an error.
    pub fn insert(&mut self, tight: &B, id: u32) -> Result<ProxyId, PhysicsError> {
        let fat = self.fatten(tight)?;
        let leaf = self.allocate().ok_or(PhysicsError::CapacityExhausted)?;
        // If the second allocation could fail, the first node would leak; it cannot (see below).
        // A tree below capacity has a leaf node and an internal node free (two nodes per proxy), so
        // the second allocation cannot fail once the first has succeeded.
        let spare = if self.root == NIL {
            NIL
        } else {
            self.allocate().ok_or(PhysicsError::CapacityExhausted)?
        };
        self.put(
            leaf,
            Node {
                bounds: fat,
                parent: NIL,
                child1: NIL,
                child2: NIL,
                height: 0,
                id,
            },
        );
        self.insert_leaf(leaf, spare);
        self.proxies += 1;
        Ok(ProxyId(leaf))
    }

    /// Removes a proxy.
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`] when the proxy names no live leaf.
    pub fn remove(&mut self, proxy: ProxyId) -> Result<(), PhysicsError> {
        let leaf = self.live_leaf(proxy)?;
        let spare = self.detach_leaf(leaf);
        if spare != NIL {
            self.release(spare);
        }
        self.release(leaf);
        self.proxies -= 1;
        Ok(())
    }

    /// Tells the tree that the object moved to the tight box `tight`. When the stored fat box still
    /// holds it nothing happens and the answer is `false`; otherwise the leaf is taken out and
    /// inserted again with a new fat box (same proxy, same id) and the answer is `true`.
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`] for a proxy that is not live; [`PhysicsError::NotFinite`] when
    /// `tight` or its fat box is not valid. Nothing changes on an error.
    pub fn update(&mut self, proxy: ProxyId, tight: &B) -> Result<bool, PhysicsError> {
        let leaf = self.live_leaf(proxy)?;
        if !tight.is_valid() {
            return Err(PhysicsError::NotFinite);
        }
        if self.node(leaf).bounds.contains(tight) {
            return Ok(false);
        }
        let fat = self.fatten(tight)?;
        let spare = self.detach_leaf(leaf);
        self.edit(leaf, |node| node.bounds = fat);
        self.insert_leaf(leaf, spare);
        Ok(true)
    }

    /// The fat box stored for a proxy.
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`].
    pub fn fat_bounds(&self, proxy: ProxyId) -> Result<B, PhysicsError> {
        self.live_leaf(proxy).map(|leaf| self.node(leaf).bounds)
    }

    /// Calls `visit` with the id of every leaf whose fat box overlaps `region`, in tree order (a
    /// function of the tree's structure, which is a function of the calls that built it).
    pub fn query(&self, region: &B, mut visit: impl FnMut(u32)) {
        let mut current = self.root;
        while current != NIL {
            current = self.query_step(current, region, &mut visit);
        }
    }

    /// Looks at one node of a query and returns the next one: into the first child of an internal
    /// node that overlaps, otherwise past the subtree (after reporting a leaf that overlaps).
    fn query_step(&self, current: u32, region: &B, visit: &mut impl FnMut(u32)) -> u32 {
        let node = self.node(current);
        if !node.bounds.overlaps(region) {
            self.after_subtree(current)
        } else if node.is_leaf() {
            visit(node.id);
            self.after_subtree(current)
        } else {
            node.child1
        }
    }

    /// A hash of the whole structure: every node (box, links, height, id) in index order, the root,
    /// the top of the free stack and the counts. Equal calls give equal hashes on every run.
    #[must_use]
    pub fn structure_hash(&self) -> u64 {
        let mut hasher = StateHasher::new();
        for value in [self.capacity, self.root, self.free, self.proxies] {
            hasher.write_u32(value);
        }
        for node in &self.nodes {
            node.write_to(&mut hasher);
        }
        hasher.finish()
    }

    // ---- node access (a missing node reads as a free one; nothing here can panic) -----------

    fn node(&self, index: u32) -> Node<B> {
        self.nodes
            .get(index as usize)
            .copied()
            .unwrap_or_else(|| Node::free(NIL))
    }

    fn put(&mut self, index: u32, node: Node<B>) {
        self.edit(index, |slot| *slot = node);
    }

    fn edit(&mut self, index: u32, change: impl FnOnce(&mut Node<B>)) {
        if let Some(node) = self.nodes.get_mut(index as usize) {
            change(node);
        }
    }

    fn set_parent(&mut self, index: u32, parent: u32) {
        self.edit(index, |node| node.parent = parent);
    }

    /// Sets the second (or the first) child link of `index`.
    fn set_child(&mut self, index: u32, second: bool, child: u32) {
        if second {
            self.edit(index, |node| node.child2 = child);
        } else {
            self.edit(index, |node| node.child1 = child);
        }
    }

    /// The leaf a live proxy names.
    fn live_leaf(&self, proxy: ProxyId) -> Result<u32, PhysicsError> {
        let node = self.node(proxy.0);
        if node.height == 0 {
            Ok(proxy.0)
        } else {
            Err(PhysicsError::StaleHandle)
        }
    }

    /// The tight box grown by the margin, or `NotFinite` when either is not a valid box.
    fn fatten(&self, tight: &B) -> Result<B, PhysicsError> {
        let fat = tight.fattened(self.margin);
        if tight.is_valid() && fat.is_valid() {
            Ok(fat)
        } else {
            Err(PhysicsError::NotFinite)
        }
    }

    fn allocate(&mut self) -> Option<u32> {
        let index = self.free;
        if index == NIL {
            return None;
        }
        self.free = self.node(index).parent;
        Some(index)
    }

    fn release(&mut self, index: u32) {
        let below = self.free;
        self.put(index, Node::free(below));
        self.free = index;
    }

    // ---- structure ----------------------------------------------------------------------------

    /// The node a walk continues with after the subtree of `current` is done: the second child of
    /// the nearest ancestor that was entered through its first child, or `NIL` at the end.
    fn after_subtree(&self, current: u32) -> u32 {
        std::iter::successors(Some(current), |&index| {
            Some(self.node(index).parent).filter(|&parent| parent != NIL)
        })
        .find_map(|index| self.second_sibling(index))
        .unwrap_or(NIL)
    }

    /// The second child of the parent of `index` when `index` is the first one.
    fn second_sibling(&self, index: u32) -> Option<u32> {
        let parent = self.node(self.node(index).parent);
        (parent.child1 == index).then_some(parent.child2)
    }

    /// Points the link that named `old` in `parent` (or the root) at `new`.
    fn replace_child(&mut self, parent: u32, old: u32, new: u32) {
        if parent == NIL {
            self.root = new;
        } else if self.node(parent).child1 == old {
            self.edit(parent, |node| node.child1 = new);
        } else {
            self.edit(parent, |node| node.child2 = new);
        }
    }

    /// The node to pair a new leaf with: descend by the surface-area heuristic, stopping at a leaf
    /// or where pairing here is cheaper than going into either child. Ties go to the second child.
    fn find_sibling(&self, leaf_bounds: &B) -> u32 {
        let mut index = self.root;
        while let Some(child) = self.descend(index, leaf_bounds) {
            index = child;
        }
        index
    }

    /// The child of `index` the new leaf should go into, or `None` to pair with `index` itself. A
    /// tie between the children goes to the second one.
    fn descend(&self, index: u32, leaf_bounds: &B) -> Option<u32> {
        let node = self.node(index);
        if node.is_leaf() {
            return None;
        }
        let combined = node.bounds.union(leaf_bounds).measure();
        let inheritance = 2.0 * (combined - node.bounds.measure());
        let cost1 = self.descent_cost(node.child1, leaf_bounds, inheritance);
        let cost2 = self.descent_cost(node.child2, leaf_bounds, inheritance);
        if 2.0 * combined < cost1 && 2.0 * combined < cost2 {
            None
        } else if cost1 < cost2 {
            Some(node.child1)
        } else {
            Some(node.child2)
        }
    }

    /// What it costs to push the new leaf into `child`: the growth of the child's box (a leaf is
    /// paired with, so its whole new box counts) plus what the ancestors inherit.
    fn descent_cost(&self, child: u32, leaf_bounds: &B, inheritance: f32) -> f32 {
        let node = self.node(child);
        let grown = node.bounds.union(leaf_bounds).measure();
        if node.is_leaf() {
            grown + inheritance
        } else {
            grown - node.bounds.measure() + inheritance
        }
    }

    /// Links `leaf` into the tree under the internal node `spare` (unused when the tree is empty).
    fn insert_leaf(&mut self, leaf: u32, spare: u32) {
        if self.root == NIL {
            self.root = leaf;
            self.set_parent(leaf, NIL);
            return;
        }
        let leaf_bounds = self.node(leaf).bounds;
        let sibling = self.find_sibling(&leaf_bounds);
        let sibling_node = self.node(sibling);
        let old_parent = sibling_node.parent;
        self.put(
            spare,
            Node {
                bounds: leaf_bounds.union(&sibling_node.bounds),
                parent: old_parent,
                child1: sibling,
                child2: leaf,
                height: sibling_node.height + 1,
                id: NIL,
            },
        );
        self.replace_child(old_parent, sibling, spare);
        self.set_parent(sibling, spare);
        self.set_parent(leaf, spare);
        self.refit(spare);
    }

    /// Takes `leaf` out of the tree and returns the internal node that held it (`NIL` when the leaf
    /// was the root); the caller releases it or reuses it.
    fn detach_leaf(&mut self, leaf: u32) -> u32 {
        if self.root == leaf {
            self.root = NIL;
            return NIL;
        }
        let parent = self.node(leaf).parent;
        let parent_node = self.node(parent);
        let sibling = if parent_node.child1 == leaf {
            parent_node.child2
        } else {
            parent_node.child1
        };
        self.replace_child(parent_node.parent, parent, sibling);
        self.set_parent(sibling, parent_node.parent);
        self.set_parent(leaf, NIL);
        self.refit(parent_node.parent);
        parent
    }

    /// Walks from `start` to the root, rebalancing each node and recomputing its box and height.
    fn refit(&mut self, start: u32) {
        let mut index = start;
        while index != NIL {
            index = self.balance(index);
            self.recompute(index);
            index = self.node(index).parent;
        }
    }

    /// Sets the box and the height of an internal node from its children.
    fn recompute(&mut self, index: u32) {
        let node = self.node(index);
        let first = self.node(node.child1);
        let second = self.node(node.child2);
        self.edit(index, |node| {
            node.bounds = first.bounds.union(&second.bounds);
            node.height = 1 + first.height.max(second.height);
        });
    }

    /// Rotates the taller child of `a` above it when the children differ in height by more than
    /// one; returns the root of the subtree.
    fn balance(&mut self, a: u32) -> u32 {
        let node = self.node(a);
        if node.is_leaf() || node.height < 2 {
            return a;
        }
        let first = self.node(node.child1).height;
        let second = self.node(node.child2).height;
        if second > first.saturating_add(1) {
            self.rotate_up(a, node.child2, node.child1, true);
            node.child2
        } else if first > second.saturating_add(1) {
            self.rotate_up(a, node.child1, node.child2, false);
            node.child1
        } else {
            a
        }
    }

    /// Makes `up` (a child of `a`, the second one when `up_is_second`) the parent of `a`. `a` keeps
    /// `stay`, and takes the shorter of the two children of `up` where `up` was; `up` keeps the
    /// taller one (the second on a tie).
    fn rotate_up(&mut self, a: u32, up: u32, stay: u32, up_is_second: bool) {
        let a_parent = self.node(a).parent;
        let up_node = self.node(up);
        let (tall, short) = if self.node(up_node.child1).height > self.node(up_node.child2).height {
            (up_node.child1, up_node.child2)
        } else {
            (up_node.child2, up_node.child1)
        };
        self.replace_child(a_parent, a, up);
        self.set_parent(up, a_parent);
        self.set_parent(a, up);
        self.set_parent(short, a);
        self.edit(up, |node| {
            node.child1 = a;
            node.child2 = tall;
        });
        self.set_child(a, up_is_second, short);
        let (stay_node, short_node) = (self.node(stay), self.node(short));
        let a_bounds = stay_node.bounds.union(&short_node.bounds);
        let a_height = 1 + stay_node.height.max(short_node.height);
        let tall_node = self.node(tall);
        self.edit(a, |node| {
            node.bounds = a_bounds;
            node.height = a_height;
        });
        self.edit(up, |node| {
            node.bounds = a_bounds.union(&tall_node.bounds);
            node.height = 1 + a_height.max(tall_node.height);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::MAX_BODIES;
    use crate::d2::Aabb2;
    use base::math::Vec2;
    use base::rng::Pcg32;

    type T = Tree<Aabb2>;

    const MARGIN: f32 = 0.25;

    fn bx(x: f32, y: f32, w: f32, h: f32) -> Aabb2 {
        Aabb2::new(Vec2::new(x, y), Vec2::new(x + w, y + h))
    }

    fn tree(capacity: u32, margin: f32) -> T {
        match Tree::new(capacity, margin) {
            Ok(tree) => tree,
            Err(error) => panic!("tree must build: {error}"),
        }
    }

    fn insert(tree: &mut T, bounds: &Aabb2, id: u32) -> ProxyId {
        match tree.insert(bounds, id) {
            Ok(proxy) => proxy,
            Err(error) => panic!("insert must succeed: {error}"),
        }
    }

    fn hits(tree: &T, region: &Aabb2) -> Vec<u32> {
        let mut found = Vec::new();
        tree.query(region, |id| found.push(id));
        found.sort_by_key(|&id| id);
        found
    }

    /// A small count as an index, saturating (no `as` cast in the tests).
    fn index(i: usize) -> u32 {
        u32::try_from(i).unwrap_or(u32::MAX)
    }

    /// A small count as a float.
    fn lane(i: u32) -> f32 {
        f32::from(u16::try_from(i).unwrap_or(u16::MAX))
    }

    /// A row of `count` boxes one apart, in a tree without a margin.
    fn row(count: u32) -> T {
        let mut t = tree(count + 4, 0.0);
        for i in 0..count {
            insert(&mut t, &bx(lane(i) * 2.0, 0.0, 1.0, 1.0), i);
        }
        t
    }

    fn big() -> Aabb2 {
        bx(-1.0e6, -1.0e6, 2.0e6, 2.0e6)
    }

    // ---- creation ---------------------------------------------------------------------------

    #[test]
    fn a_new_tree_is_empty_and_keeps_its_capacity_and_margin() {
        let t = tree(8, MARGIN);
        assert_eq!(
            (t.capacity(), t.len(), t.is_empty(), t.height()),
            (8, 0, true, 0)
        );
        assert!((t.margin() - MARGIN).abs() < f32::EPSILON);
        assert!(hits(&t, &big()).is_empty());
    }

    #[test]
    fn a_capacity_above_the_maximum_is_refused() {
        assert_eq!(
            T::new(MAX_BODIES + 1, MARGIN).err(),
            Some(PhysicsError::CapacityTooLarge { max: MAX_BODIES })
        );
        assert!(T::new(MAX_BODIES, MARGIN).is_ok());
    }

    #[test]
    fn a_margin_that_is_negative_or_not_finite_is_refused() {
        for margin in [-0.1, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(T::new(4, margin).err(), Some(PhysicsError::InvalidMargin));
        }
        assert!(T::new(4, 0.0).is_ok());
    }

    #[test]
    fn a_tree_of_zero_capacity_refuses_every_proxy() {
        let mut t = tree(0, MARGIN);
        assert_eq!(
            t.insert(&bx(0.0, 0.0, 1.0, 1.0), 0).err(),
            Some(PhysicsError::CapacityExhausted)
        );
        assert!(t.is_empty());
    }

    // ---- insert and query -------------------------------------------------------------------

    #[test]
    fn one_proxy_is_found_by_an_overlapping_region_only() {
        let mut t = tree(4, MARGIN);
        let proxy = insert(&mut t, &bx(0.0, 0.0, 1.0, 1.0), 7);
        assert_eq!((t.len(), t.height()), (1, 0));
        assert_eq!(hits(&t, &bx(0.5, 0.5, 1.0, 1.0)), vec![7]);
        assert!(hits(&t, &bx(5.0, 5.0, 1.0, 1.0)).is_empty());
        assert_eq!(
            t.fat_bounds(proxy),
            Ok(bx(-MARGIN, -MARGIN, 1.0 + 2.0 * MARGIN, 1.0 + 2.0 * MARGIN))
        );
    }

    #[test]
    fn a_tree_with_a_proxy_is_not_empty_and_proxies_have_their_own_indices() {
        let mut t = tree(4, MARGIN);
        let a = insert(&mut t, &bx(0.0, 0.0, 1.0, 1.0), 0);
        assert!(!t.is_empty());
        let b = insert(&mut t, &bx(5.0, 0.0, 1.0, 1.0), 1);
        assert_ne!(a.index(), b.index());
        assert_ne!(a, b);
        assert_eq!(t.node(a.index()).id, 0);
        assert_eq!(t.node(b.index()).id, 1);
    }

    #[test]
    fn the_stored_box_is_the_tight_box_grown_by_the_margin() {
        let mut t = tree(4, 0.5);
        let proxy = insert(&mut t, &bx(1.0, 2.0, 2.0, 2.0), 0);
        assert_eq!(t.fat_bounds(proxy), Ok(bx(0.5, 1.5, 3.0, 3.0)));
    }

    #[test]
    fn regions_that_touch_a_stored_box_exactly_find_it_and_regions_with_a_gap_do_not() {
        let mut t = tree(4, 0.0);
        insert(&mut t, &bx(0.0, 0.0, 1.0, 1.0), 1);
        assert_eq!(hits(&t, &bx(1.0, 0.0, 1.0, 1.0)), vec![1]);
        assert_eq!(hits(&t, &bx(1.0, 1.0, 1.0, 1.0)), vec![1]);
        assert!(hits(&t, &bx(1.001, 0.0, 1.0, 1.0)).is_empty());
    }

    #[test]
    fn a_box_that_is_not_valid_or_overflows_when_fattened_is_refused_and_changes_nothing() {
        let mut t = tree(4, 1.0);
        let before = t.structure_hash();
        let inverted = Aabb2::new(Vec2::new(1.0, 1.0), Vec2::new(0.0, 0.0));
        for bad in [inverted, bx(f32::NAN, 0.0, 1.0, 1.0)] {
            assert_eq!(t.insert(&bad, 0).err(), Some(PhysicsError::NotFinite));
        }
        assert_eq!(t.structure_hash(), before);
        assert!(t.is_empty());
        // A valid box whose fat box reaches past the largest float.
        let mut wide = tree(4, f32::MAX);
        let edge = Aabb2::new(Vec2::new(0.0, 0.0), Vec2::new(f32::MAX, 1.0));
        assert_eq!(wide.insert(&edge, 0).err(), Some(PhysicsError::NotFinite));
        assert!(wide.is_empty());
    }

    #[test]
    fn the_tree_refuses_the_proxy_past_its_capacity_and_takes_it_after_a_removal() {
        let mut t = tree(3, MARGIN);
        let proxies: Vec<ProxyId> = (0..3)
            .map(|i| insert(&mut t, &bx(lane(i) * 3.0, 0.0, 1.0, 1.0), i))
            .collect();
        let full = t.structure_hash();
        assert_eq!(
            t.insert(&bx(20.0, 0.0, 1.0, 1.0), 9).err(),
            Some(PhysicsError::CapacityExhausted)
        );
        assert_eq!(t.structure_hash(), full);
        assert_eq!(t.remove(proxies[1]), Ok(()));
        insert(&mut t, &bx(20.0, 0.0, 1.0, 1.0), 9);
        assert_eq!(hits(&t, &big()), vec![0, 2, 9]);
    }

    // ---- remove and update ------------------------------------------------------------------

    #[test]
    fn removing_a_proxy_twice_or_a_proxy_that_never_existed_is_a_stale_handle() {
        let mut t = tree(4, MARGIN);
        let proxy = insert(&mut t, &bx(0.0, 0.0, 1.0, 1.0), 0);
        assert_eq!(t.remove(proxy), Ok(()));
        assert_eq!(t.remove(proxy), Err(PhysicsError::StaleHandle));
        assert_eq!(t.remove(ProxyId(999)), Err(PhysicsError::StaleHandle));
        assert_eq!(t.fat_bounds(proxy), Err(PhysicsError::StaleHandle));
        assert_eq!(
            t.update(proxy, &bx(0.0, 0.0, 1.0, 1.0)),
            Err(PhysicsError::StaleHandle)
        );
        assert!(t.is_empty());
    }

    #[test]
    fn an_update_inside_the_fat_box_changes_nothing_and_one_outside_moves_the_proxy() {
        let mut t = tree(4, 0.5);
        let proxy = insert(&mut t, &bx(0.0, 0.0, 1.0, 1.0), 3);
        insert(&mut t, &bx(10.0, 0.0, 1.0, 1.0), 4);
        let before = t.structure_hash();
        assert_eq!(t.update(proxy, &bx(0.25, 0.25, 1.0, 1.0)), Ok(false));
        assert_eq!(t.structure_hash(), before);
        assert_eq!(t.update(proxy, &bx(10.0, 5.0, 1.0, 1.0)), Ok(true));
        assert_eq!(t.fat_bounds(proxy), Ok(bx(9.5, 4.5, 2.0, 2.0)));
        assert_eq!(hits(&t, &bx(10.0, 5.0, 1.0, 1.0)), vec![3]);
        assert_eq!(hits(&t, &bx(0.0, 0.0, 1.0, 1.0)), Vec::<u32>::new());
    }

    #[test]
    fn an_update_to_a_box_that_is_not_valid_is_refused_and_changes_nothing() {
        let mut t = tree(4, 0.5);
        let proxy = insert(&mut t, &bx(0.0, 0.0, 1.0, 1.0), 3);
        let before = t.structure_hash();
        assert_eq!(
            t.update(proxy, &bx(f32::NAN, 0.0, 1.0, 1.0)),
            Err(PhysicsError::NotFinite)
        );
        assert_eq!(t.structure_hash(), before);
    }

    // ---- determinism ------------------------------------------------------------------------

    #[test]
    fn the_same_calls_give_the_same_structure_and_a_changed_box_or_id_a_different_one() {
        let build = |x: f32, id: u32| {
            let mut t = row(5);
            insert(&mut t, &bx(x, 0.0, 1.0, 1.0), id);
            t.structure_hash()
        };
        assert_eq!(build(20.0, 9), build(20.0, 9));
        assert_ne!(build(20.0, 9), build(21.0, 9));
        assert_ne!(build(20.0, 9), build(20.0, 8));
    }

    // ---- random sequences against the brute-force oracle -----------------------------------

    fn random_box(rng: &mut Pcg32) -> Aabb2 {
        bx(
            rng.next_f32() * 40.0,
            rng.next_f32() * 40.0,
            0.1 + rng.next_f32() * 4.0,
            0.1 + rng.next_f32() * 4.0,
        )
    }

    #[test]
    fn queries_equal_the_brute_force_oracle_after_random_inserts() {
        for seed in 0..20_u64 {
            oracle_round(seed);
        }
    }

    fn oracle_round(seed: u64) {
        let mut rng = Pcg32::new(seed, 1);
        let count = u32::try_from(seed * 7 % 60).unwrap_or(0);
        let mut t = tree(64, MARGIN);
        let mut shadow = Vec::new();
        for id in 0..count {
            let proxy = insert(&mut t, &random_box(&mut rng), id);
            shadow.push((id, t.fat_bounds(proxy).unwrap_or_default()));
        }
        for _ in 0..30 {
            let region = random_box(&mut rng);
            let expected: Vec<u32> = shadow
                .iter()
                .filter(|(_, fat)| fat.overlaps(&region))
                .map(|(id, _)| *id)
                .collect();
            assert_eq!(hits(&t, &region), expected, "seed {seed}");
        }
    }

    // ---- the structural validator (test only) -----------------------------------------------

    /// Checks every invariant of the tree and returns (leaves, height). Walks from the root and
    /// checks the free stack, so a node that is neither reachable nor free is found too.
    fn validate(t: &T) -> Result<(usize, u32), String> {
        let mut seen = vec![false; t.nodes.len()];
        let (leaves, height) = walk_root(t, &mut seen)?;
        if leaves != t.len() {
            return Err(format!("{leaves} leaves reachable, {} counted", t.len()));
        }
        let free = walk_free(t, &mut seen)?;
        let used = if leaves == 0 { 0 } else { 2 * leaves - 1 };
        if used + free != t.nodes.len() || seen.iter().any(|s| !s) {
            return Err(format!(
                "{used} used + {free} free != {} nodes",
                t.nodes.len()
            ));
        }
        Ok((leaves, height))
    }

    /// Walks the tree from the root (an empty tree has 0 leaves and height 0).
    fn walk_root(t: &T, seen: &mut [bool]) -> Result<(usize, u32), String> {
        if t.root == NIL {
            return Ok((0, 0));
        }
        if t.node(t.root).parent != NIL {
            return Err("the root has a parent".into());
        }
        walk(t, t.root, seen)
    }

    /// Walks the free stack, marking its nodes, and returns how many there are.
    fn walk_free(t: &T, seen: &mut [bool]) -> Result<usize, String> {
        let mut free = 0;
        let mut index = t.free;
        while index != NIL {
            mark(seen, index).ok_or_else(|| format!("free node {index} is used or repeated"))?;
            let marked = t.node(index).height == FREE_HEIGHT;
            ensure(marked, format!("free node {index} is not marked free"))?;
            free += 1;
            index = t.node(index).parent;
        }
        Ok(free)
    }

    /// An error with the message when the condition does not hold.
    fn ensure(holds: bool, message: String) -> Result<(), String> {
        if holds { Ok(()) } else { Err(message) }
    }

    /// Marks a node as seen; an error when it is out of range or was seen before.
    fn mark(seen: &mut [bool], index: u32) -> Option<()> {
        let flag = seen.get_mut(index as usize).filter(|flag| !**flag)?;
        *flag = true;
        Some(())
    }

    /// A node without two children must be a proper leaf.
    fn check_leaf(index: u32, node: &Node<Aabb2>) -> Result<(usize, u32), String> {
        let leaf = node.child1 == NIL && node.child2 == NIL && node.height == 0 && node.id != NIL;
        if leaf {
            Ok((1, 0))
        } else {
            Err(format!("node {index} is a broken leaf"))
        }
    }

    fn walk(t: &T, index: u32, seen: &mut [bool]) -> Result<(usize, u32), String> {
        let node = t.node(index);
        mark(seen, index)
            .ok_or_else(|| format!("node {index} is out of range or reached twice"))?;
        if !node.bounds.is_valid() {
            return Err(format!("node {index} has an invalid box"));
        }
        if node.child1 == NIL || node.child2 == NIL {
            return check_leaf(index, &node);
        }
        for child in [node.child1, node.child2] {
            let linked = t.node(child).parent == index;
            ensure(
                linked,
                format!("child {child} does not point back to {index}"),
            )?;
        }
        let (n1, h1) = walk(t, node.child1, seen)?;
        let (n2, h2) = walk(t, node.child2, seen)?;
        if node.height != 1 + h1.max(h2) {
            return Err(format!(
                "node {index} has height {}, children {h1} and {h2}",
                node.height
            ));
        }
        if h1.abs_diff(h2) > 1 {
            return Err(format!("node {index} is unbalanced: {h1} against {h2}"));
        }
        let union = t
            .node(node.child1)
            .bounds
            .union(&t.node(node.child2).bounds);
        if node.bounds != union {
            return Err(format!("node {index} is not the union of its children"));
        }
        Ok((n1 + n2, node.height))
    }

    fn assert_valid(t: &T) -> (usize, u32) {
        match validate(t) {
            Ok(stats) => stats,
            Err(message) => panic!("tree invariant broken: {message}"),
        }
    }

    /// An upper bound on the height of an AVL-balanced tree of `n` leaves: 1.44 log2(n + 2).
    fn height_bound(n: usize) -> u32 {
        let floor_log2 = usize::BITS - 1 - (n + 2).leading_zeros();
        floor_log2 * 144 / 100 + 1
    }

    fn built(count: u32, seed: u64, margin: f32) -> (T, Vec<ProxyId>) {
        let mut rng = Pcg32::new(seed, 5);
        let mut t = tree(count + 4, margin);
        let proxies = (0..count)
            .map(|id| insert(&mut t, &random_box(&mut rng), id))
            .collect();
        (t, proxies)
    }

    fn first_leaf(t: &T) -> u32 {
        (0..index(t.nodes.len()))
            .find(|&i| t.node(i).height == 0)
            .unwrap_or(NIL)
    }

    #[test]
    fn the_validator_accepts_trees_of_every_small_size() {
        for count in 0..40 {
            let (t, _) = built(count, u64::from(count), MARGIN);
            assert_eq!(assert_valid(&t).0, count as usize);
        }
    }

    #[test]
    fn the_validator_finds_each_kind_of_damage() {
        type Damage = (&'static str, fn(&mut T));
        let damage: [Damage; 8] = [
            ("parent", |t| {
                let c = t.node(t.root).child1;
                t.set_parent(c, NIL);
            }),
            ("height", |t| {
                let r = t.root;
                t.edit(r, |n| n.height += 1);
            }),
            ("union", |t| {
                let r = t.root;
                t.edit(r, |n| n.bounds = Aabb2::default());
            }),
            ("leaf id", |t| {
                let leaf = first_leaf(t);
                t.edit(leaf, |n| n.id = NIL);
            }),
            ("free stack", |t| t.free = t.root),
            ("count", |t| t.proxies += 1),
            ("lost node", |t| t.free = NIL),
            ("unbalanced", |t| {
                let r = t.root;
                t.edit(r, |n| n.height += 2);
                let c = t.node(r).child1;
                t.edit(c, |n| n.height += 5);
            }),
        ];
        for (name, hurt) in damage {
            let (mut t, _) = built(9, 3, MARGIN);
            assert!(validate(&t).is_ok());
            hurt(&mut t);
            assert!(validate(&t).is_err(), "damage {name} must be found");
        }
    }

    // ---- balance ------------------------------------------------------------------------------

    #[test]
    fn a_row_of_boxes_inserted_in_order_stays_within_the_balance_bound() {
        for count in [1_u32, 2, 3, 4, 7, 8, 15, 16, 100, 257] {
            let t = row(count);
            let (leaves, height) = assert_valid(&t);
            assert_eq!(leaves, count as usize);
            assert!(
                height <= height_bound(leaves),
                "{count} leaves, height {height}"
            );
        }
    }

    #[test]
    fn the_same_box_many_times_still_gives_a_valid_balanced_tree() {
        let mut t = tree(64, 0.0);
        for id in 0..64 {
            insert(&mut t, &bx(1.0, 1.0, 1.0, 1.0), id);
        }
        let (_, height) = assert_valid(&t);
        assert!(height <= height_bound(64));
        assert_eq!(hits(&t, &bx(1.5, 1.5, 0.1, 0.1)).len(), 64);
    }

    #[test]
    fn removing_every_proxy_in_any_order_keeps_the_tree_valid_and_ends_empty() {
        for seed in 0..10_u64 {
            remove_all_in_random_order(seed);
        }
    }

    fn remove_all_in_random_order(seed: u64) {
        let (mut t, mut proxies) = built(50, seed, MARGIN);
        let mut rng = Pcg32::new(seed, 9);
        while !proxies.is_empty() {
            let proxy = proxies.swap_remove(pick(&mut rng, proxies.len()));
            assert_eq!(t.remove(proxy), Ok(()));
            let (leaves, height) = assert_valid(&t);
            assert_eq!(leaves, proxies.len());
            assert!(height <= height_bound(leaves));
        }
        assert!(t.is_empty());
        assert_eq!(t.height(), 0);
    }

    #[test]
    fn a_freed_node_is_the_next_one_reused() {
        let mut t = tree(4, MARGIN);
        let a = insert(&mut t, &bx(0.0, 0.0, 1.0, 1.0), 0);
        let b = insert(&mut t, &bx(5.0, 0.0, 1.0, 1.0), 1);
        assert_eq!(t.remove(b), Ok(()));
        let again = insert(&mut t, &bx(9.0, 0.0, 1.0, 1.0), 2);
        assert_eq!(again, b);
        assert_ne!(again, a);
        assert_valid(&t);
    }

    // ---- random operation sequences against a shadow list ---------------------------------

    type Shadow = Vec<(ProxyId, u32, Aabb2)>;

    fn pick(rng: &mut Pcg32, len: usize) -> usize {
        usize::try_from(rng.below(index(len)).unwrap_or(0)).unwrap_or(0)
    }

    /// One random operation on the tree and on the shadow list `(proxy, id, fat box)`.
    fn random_operation(t: &mut T, shadow: &mut Shadow, rng: &mut Pcg32, next_id: &mut u32) {
        let choice = rng.below(10).unwrap_or(0);
        if choice < 4 && shadow.len() < t.capacity() {
            let proxy = insert(t, &random_box(rng), *next_id);
            shadow.push((proxy, *next_id, t.fat_bounds(proxy).unwrap_or_default()));
            *next_id += 1;
        } else if choice < 6 && !shadow.is_empty() {
            let (proxy, _, _) = shadow.swap_remove(pick(rng, shadow.len()));
            assert_eq!(t.remove(proxy), Ok(()));
        } else if !shadow.is_empty() {
            move_one(t, shadow, rng);
        }
    }

    /// Moves one random proxy, to a random box or by a small step.
    fn move_one(t: &mut T, shadow: &mut Shadow, rng: &mut Pcg32) {
        let at = pick(rng, shadow.len());
        let (proxy, _, old) = shadow[at];
        let tight = if rng.below(2).unwrap_or(0) == 0 {
            random_box(rng)
        } else {
            // A small move of the old box: usually still inside the fat box.
            bx(old.min.x + 0.1, old.min.y, 0.5, 0.5)
        };
        assert!(t.update(proxy, &tight).is_ok());
        shadow[at].2 = t.fat_bounds(proxy).unwrap_or_default();
    }

    #[test]
    fn random_operations_keep_every_invariant_and_every_query_exact() {
        for seed in 0..60_u64 {
            random_operations_round(seed);
        }
    }

    fn random_operations_round(seed: u64) {
        let mut rng = Pcg32::new(seed, 11);
        let mut t = tree(40, if seed.is_multiple_of(3) { 0.0 } else { MARGIN });
        let mut shadow = Vec::new();
        let mut next_id = 0;
        for step in 0..150 {
            random_operation(&mut t, &mut shadow, &mut rng, &mut next_id);
            let (leaves, height) = assert_valid(&t);
            assert_eq!(leaves, shadow.len(), "seed {seed} step {step}");
            assert!(height <= height_bound(leaves));
            let region = random_box(&mut rng);
            let mut expected: Vec<u32> = shadow
                .iter()
                .filter(|e| e.2.overlaps(&region))
                .map(|e| e.1)
                .collect();
            expected.sort_by_key(|&id| id);
            assert_eq!(hits(&t, &region), expected, "seed {seed} step {step}");
        }
    }

    #[test]
    fn the_same_random_sequence_twice_gives_the_same_tree() {
        assert_eq!(run_random(4), run_random(4));
        assert_ne!(run_random(4), run_random(5));
    }

    fn run_random(seed: u64) -> u64 {
        let mut rng = Pcg32::new(seed, 11);
        let mut t = tree(40, MARGIN);
        let (mut shadow, mut next_id) = (Vec::new(), 0);
        for _ in 0..200 {
            random_operation(&mut t, &mut shadow, &mut rng, &mut next_id);
        }
        t.structure_hash()
    }

    /// A tree holding `boxes[i]` (with id `i`) for each `i` of `order`, in that order.
    fn tree_in_order(boxes: &[Aabb2], order: &[usize]) -> T {
        let mut t = tree(30, MARGIN);
        for &i in order {
            insert(&mut t, &boxes[i], index(i));
        }
        t
    }

    /// The structure hashes of fixed scenarios. They pin the choices that no query can see (which
    /// sibling a leaf is paired with, which child a rotation lifts, how ties break), so that a change
    /// of those choices is a visible decision, and they are the same on every platform.
    fn pinned_scenarios() -> [u64; 6] {
        let (grown, _) = built(30, 8, MARGIN);
        let (flat, _) = built(17, 2, 0.0);
        let (mut shrunk, mut proxies) = built(25, 4, MARGIN);
        for proxy in proxies.split_off(15).into_iter().rev() {
            assert_eq!(shrunk.remove(proxy), Ok(()));
        }
        [
            row(7).structure_hash(),
            row(16).structure_hash(),
            grown.structure_hash(),
            flat.structure_hash(),
            shrunk.structure_hash(),
            run_random(4),
        ]
    }

    #[test]
    fn the_structure_of_fixed_scenarios_is_pinned() {
        assert_eq!(
            pinned_scenarios(),
            [
                0x5fcc_d2ad_b676_9627,
                0x2a9d_a794_4172_e02e,
                0xb6bd_75f6_0b2e_4697,
                0x9a14_923e_0bad_fbb9,
                0x9510_e467_337e_8249,
                0x6641_d879_0d30_455a
            ]
        );
        let t = row(7);
        assert!(t.height() >= 2 && t.height() <= height_bound(7));
    }

    #[test]
    fn the_result_set_of_a_query_does_not_depend_on_the_order_of_insertion() {
        let mut rng = Pcg32::new(77, 3);
        let boxes: Vec<Aabb2> = (0..30).map(|_| random_box(&mut rng)).collect();
        let query_all = |order: &[usize]| {
            let t = tree_in_order(&boxes, order);
            assert_valid(&t);
            boxes.iter().map(|b| hits(&t, b)).collect::<Vec<_>>()
        };
        let forward: Vec<usize> = (0..30).collect();
        let backward: Vec<usize> = (0..30).rev().collect();
        let interleaved: Vec<usize> = (0..15).flat_map(|i| [i, 29 - i]).collect();
        let reference = query_all(&forward);
        assert_eq!(query_all(&backward), reference);
        assert_eq!(query_all(&interleaved), reference);
    }

    #[test]
    fn a_moved_tree_finds_what_a_tree_built_from_scratch_on_the_same_fat_boxes_finds() {
        let mut rng = Pcg32::new(5, 5);
        let (mut t, proxies) = built(30, 5, MARGIN);
        for _ in 0..100 {
            let at = pick(&mut rng, 30);
            assert!(t.update(proxies[at], &random_box(&mut rng)).is_ok());
        }
        assert_valid(&t);
        let mut fresh = tree(30, 0.0);
        for (id, proxy) in proxies.iter().enumerate() {
            let fat = t.fat_bounds(*proxy).unwrap_or_default();
            insert(&mut fresh, &fat, index(id));
        }
        for _ in 0..50 {
            let region = random_box(&mut rng);
            assert_eq!(hits(&t, &region), hits(&fresh, &region));
        }
    }
}
