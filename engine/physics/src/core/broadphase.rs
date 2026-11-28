//! The broadphase of a world: one tree for the static bodies, one for the dynamic bodies, and the
//! pair list built from them (design: `wiki/architecture/physics-and-jobs-v0.md` section 3).
//!
//! Static bodies sit in a tree of their own, without a margin, because they never move; dynamic
//! bodies sit in a tree whose boxes are grown by the margin. A pair is looked for from the dynamic
//! side only: each dynamic body asks the dynamic tree (for the dynamic bodies with a higher slot,
//! so that a pair is found once) and the static tree (for all of them). Two static bodies are never
//! asked about each other. The finished list is sorted into the canonical order of [`PairList`].

use super::body::BodyKind;
use super::bounds::Bounds;
use super::dim::Dim;
use super::error::PhysicsError;
use super::execution::SolverExecution;
use super::hash::StateHasher;
use super::pair_run::{PairRun, Search};
use super::pairs::PairList;
use super::tree::{ProxyId, Tree};
use super::world::Slot;
use base::fpenv::FpGuard;

/// The two trees and the pair list of a world.
pub(super) struct Broadphase<D: Dim> {
    static_tree: Tree<D::Bounds>,
    dynamic_tree: Tree<D::Bounds>,
    pairs: PairList<D>,
    /// The pair search through a scheduler, when the execution has one (card 8-111).
    run: Option<PairRun<D>>,
}

impl<D: Dim> Broadphase<D> {
    /// Builds both trees for `max_bodies` bodies and a pair list for `max_pairs` pairs.
    pub(super) fn new(max_bodies: u32, margin: f32, max_pairs: u32) -> Result<Self, PhysicsError> {
        Ok(Self {
            // Static bodies never move, so their boxes are stored as they are, without a margin.
            static_tree: Tree::new(max_bodies, 0.0)?,
            dynamic_tree: Tree::new(max_bodies, margin)?,
            pairs: PairList::new(max_pairs)?,
            run: None,
        })
    }

    fn tree_mut(&mut self, kind: BodyKind) -> &mut Tree<D::Bounds> {
        match kind {
            BodyKind::Static => &mut self.static_tree,
            BodyKind::Dynamic => &mut self.dynamic_tree,
        }
    }

    /// True when the trees can hold a body of `kind` whose tight box is `tight`: the box is valid
    /// and so is the box grown by the margin of the tree of that kind. Checked before a body is
    /// created or moved, so that [`Broadphase::insert`] and [`Broadphase::move_proxies`] cannot fail
    /// half way.
    pub(super) fn can_hold(&self, kind: BodyKind, tight: &D::Bounds) -> bool {
        let margin = match kind {
            BodyKind::Static => 0.0,
            BodyKind::Dynamic => self.dynamic_tree.margin(),
        };
        tight.is_valid() && tight.fattened(margin).is_valid()
    }

    /// The pairs found by the last [`Broadphase::find_pairs`].
    pub(super) fn pairs(&self) -> &PairList<D> {
        &self.pairs
    }

    /// Adds the box of a body of `kind` that lives in slot `id`.
    pub(super) fn insert(
        &mut self,
        kind: BodyKind,
        tight: &D::Bounds,
        id: u32,
    ) -> Result<ProxyId, PhysicsError> {
        self.tree_mut(kind).insert(tight, id)
    }

    /// Takes the box of a body out.
    pub(super) fn remove(&mut self, kind: BodyKind, proxy: ProxyId) -> Result<(), PhysicsError> {
        self.tree_mut(kind).remove(proxy)
    }

    /// The box stored for a body.
    pub(super) fn bounds(&self, kind: BodyKind, proxy: ProxyId) -> Result<D::Bounds, PhysicsError> {
        match kind {
            BodyKind::Static => self.static_tree.fat_bounds(proxy),
            BodyKind::Dynamic => self.dynamic_tree.fat_bounds(proxy),
        }
    }

    /// A hash of the structure of both trees.
    pub(super) fn structure_hash(&self) -> u64 {
        let mut hasher = StateHasher::new();
        hasher.write_u64(self.static_tree.structure_hash());
        hasher.write_u64(self.dynamic_tree.structure_hash());
        hasher.finish()
    }

    /// Tells the dynamic tree where the dynamic bodies of `slots` are now. The caller has checked
    /// every box with [`Broadphase::can_hold`], so no update fails.
    pub(super) fn move_proxies(&mut self, slots: &[Option<Slot<D>>]) -> Result<(), PhysicsError> {
        slots
            .iter()
            .flatten()
            .filter(|slot| slot.body.kind() == BodyKind::Dynamic)
            .try_for_each(|slot| {
                self.dynamic_tree
                    .update(slot.proxy, &slot.body.bounds())
                    .map(drop)
            })
    }

    /// Chooses whether the pair search runs through a scheduler (see [`SolverExecution`]) for a
    /// world of `max_bodies` slots. The pairs found are the same either way.
    pub(super) fn set_execution(&mut self, execution: &SolverExecution, max_bodies: usize) {
        self.run = PairRun::new(execution, max_bodies, self.pairs.capacity());
    }

    /// Builds the pair list from the boxes the trees hold now.
    pub(super) fn find_pairs(&mut self, slots: &[Option<Slot<D>>]) {
        // The same floating-point state on every path: a scheduled chunk takes it, so the serial
        // search must too, whatever the caller left in the control registers.
        let _guard = FpGuard::deterministic();
        self.pairs.clear();
        let search = Search {
            dynamic_tree: &self.dynamic_tree,
            static_tree: &self.static_tree,
            slots,
        };
        let parallel = self
            .run
            .as_ref()
            .is_some_and(|run| run.run(&search, &mut self.pairs));
        if !parallel {
            search.serial(&mut self.pairs);
        }
        self.pairs.sort();
    }
}
