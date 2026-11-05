//! The world: fixed-capacity body storage with generation-checked handles, the step and the state
//! hash. Dimension-generic; the instance supplies the types through [`Dim`].
//!
//! Capacity is fixed when the world is built. Handles are issued by a `base::pool::Pool` (index
//! order, last freed first, generation checked); the bodies live in a plain vector of the same
//! length, so iteration is in slot-index order and nothing allocates after [`World::new`].

use super::MAX_BODIES;
use super::body::{Body, BodyDesc, BodyHandle, BodyKind, BodyTag};
use super::broadphase::Broadphase;
use super::dim::{Dim, Quantity};
use super::error::PhysicsError;
use super::execution::SolverExecution;
use super::hash::{HashBits, StateHasher};
use super::islands::Islands;
use super::joint::{JointDesc, JointEntry, JointHandle, JointTable};
use super::manifolds::{ContactManifold, ManifoldList};
use super::pairs::{Pair, PairList};
use super::sleep::SleepConfig;
use super::solver::{ContactSolver, SolverConfig, SolverStats, StepFrame};
use super::tree::ProxyId;
use base::pool::Pool;

/// The margin, in metres, a fat box is grown by when the configuration does not say otherwise: a
/// body that moves less than this since its box was last stored costs the tree nothing.
pub const DEFAULT_BROADPHASE_MARGIN: f32 = 0.1;

/// The distance, in metres, within which two shapes that do not touch yet still get a contact (a
/// speculative contact, for bodies that would move into each other within the step): 2 centimetres,
/// four times the slop of `Box2D` v3.
pub const DEFAULT_SPECULATIVE_MARGIN: f32 = 0.02;

/// What a world is built from. The capacities are fixed for the life of the world.
pub struct WorldConfig<D: Dim> {
    /// The most bodies alive at once, at most [`MAX_BODIES`].
    pub max_bodies: u32,
    /// Acceleration of gravity in metres per second squared.
    pub gravity: D::Vec,
    /// The most overlapping pairs the pair list holds, at most [`MAX_PAIRS`](super::MAX_PAIRS).
    /// Pairs past it are counted, not stored (see [`PairList`]).
    pub max_pairs: u32,
    /// How far, in metres, the broadphase grows a body's box on every side, zero or more
    /// ([`DEFAULT_BROADPHASE_MARGIN`]). Pairs are pairs of grown boxes, so a larger margin finds
    /// more pairs and re-inserts a moving body less often.
    pub broadphase_margin: f32,
    /// The most manifolds the manifold list holds, at most [`MAX_PAIRS`](super::MAX_PAIRS). A
    /// manifold exists only for a pair whose shapes touch or are within the speculative margin, so
    /// this is normally far below `max_pairs`. Manifolds past it are counted, not stored (see
    /// [`ManifoldList`]).
    pub max_manifolds: u32,
    /// The most joints alive at once, at most [`MAX_JOINTS`](super::MAX_JOINTS); zero for a world
    /// without joints. Creating a joint past it is an error value, never a reallocation.
    pub max_joints: u32,
    /// The distance, in metres, up to which shapes that do not touch still get a contact, zero or
    /// more ([`DEFAULT_SPECULATIVE_MARGIN`]). A pair is a candidate only when the fat boxes of the
    /// broadphase overlap, so a margin above the broadphase margin is not reached for every pair; it
    /// is not checked. A box pair can also give no contact inside the margin when the closest
    /// corners are further apart than the margin (the separating axes alone are within it).
    pub speculative_margin: f32,
    /// The tunables of the contact solver ([`SolverConfig::DEFAULT`]).
    pub solver: SolverConfig,
    /// The tunables of sleeping ([`SleepConfig::DEFAULT`]: on, 0.5 s, 0.05 m/s, 2 degrees per
    /// second). A sleeping island is not moved by the step; see [`SleepConfig`].
    pub sleep: SleepConfig,
}

// Written by hand: `#[derive]` would add the bound `D: Clone` (ADR-0018, Rust friction 3).
impl<D: Dim> Clone for WorldConfig<D> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<D: Dim> Copy for WorldConfig<D> {}

/// A live body and the handle that names it.
pub(super) struct Slot<D: Dim> {
    pub(super) handle: BodyHandle<D>,
    pub(super) body: Body<D>,
    /// The body's leaf in the static or the dynamic tree, by its kind. The leaf id is the slot index.
    pub(super) proxy: ProxyId,
}

/// A rigid-body world.
pub struct World<D: Dim> {
    handles: Pool<BodyTag<D>>,
    slots: Vec<Option<Slot<D>>>,
    capacity: u32,
    gravity: D::Vec,
    steps: u64,
    broadphase: Broadphase<D>,
    manifolds: ManifoldList<D>,
    joints: JointTable<D>,
    speculative_margin: f32,
    solver: D::Solver,
    sleep: SleepConfig,
    /// The islands of the manifolds of the last step and the rest timers of the bodies.
    islands: Islands,
    /// Copies of the bodies the solver advances; the step commits them only when all are valid.
    scratch: Vec<Option<Body<D>>>,
    /// True when bodies were created or destroyed since the pairs and manifolds were made.
    contacts_stale: bool,
}

/// The live body `handle` names in `slots`, or `StaleHandle`.
pub(super) fn live_slot<D: Dim>(
    slots: &[Option<Slot<D>>],
    handle: BodyHandle<D>,
) -> Result<&Slot<D>, PhysicsError> {
    slots
        .get(handle.index() as usize)
        .and_then(Option::as_ref)
        .filter(|slot| slot.handle == handle)
        .ok_or(PhysicsError::StaleHandle)
}

/// Copies the body the solver advanced into its slot; an empty slot or copy changes nothing.
fn store<D: Dim>(slot: &mut Option<Slot<D>>, copy: Option<&Body<D>>) {
    if let (Some(slot), Some(copy)) = (slot.as_mut(), copy) {
        slot.body = *copy;
    }
}

/// The manifold of `pair`, or `None` when a body of the pair is gone or the shapes do not touch.
fn contact_of<D: Dim>(
    slots: &[Option<Slot<D>>],
    joints: &JointTable<D>,
    pair: &Pair<D>,
    margin: f32,
) -> Option<ContactManifold<D>> {
    if joints.forbids_contact(pair.first.index(), pair.second.index()) {
        return None;
    }
    let first = live_slot(slots, pair.first).ok()?;
    let second = live_slot(slots, pair.second).ok()?;
    let manifold = D::collide(&first.body.placed(), &second.body.placed(), margin)?;
    Some(ContactManifold {
        first: first.handle,
        second: second.handle,
        manifold,
    })
}

/// Wakes every sleeping body in the island of the body in `index`; the rest timers restart.
fn wake_island_in<D: Dim>(slots: &mut [Option<Slot<D>>], islands: &mut Islands, index: usize) {
    let Some(label) = islands.label_of(index) else {
        return;
    };
    for (other, slot) in slots
        .iter_mut()
        .enumerate()
        .filter_map(|(other, slot)| slot.as_mut().map(|slot| (other, slot)))
        .filter(|(_, slot)| slot.body.is_asleep())
    {
        if islands.label_of(other) == Some(label) {
            slot.body.wake_up();
            islands.restart(other);
        }
    }
}

/// The other body of `entry` when `handle` is one of its two bodies.
fn partner<D: Dim>(entry: &ContactManifold<D>, handle: BodyHandle<D>) -> Option<BodyHandle<D>> {
    if entry.first == handle {
        Some(entry.second)
    } else if entry.second == handle {
        Some(entry.first)
    } else {
        None
    }
}

/// Mutable form of [`live_slot`].
fn live_slot_mut<D: Dim>(
    slots: &mut [Option<Slot<D>>],
    handle: BodyHandle<D>,
) -> Result<&mut Slot<D>, PhysicsError> {
    slots
        .get_mut(handle.index() as usize)
        .and_then(Option::as_mut)
        .filter(|slot| slot.handle == handle)
        .ok_or(PhysicsError::StaleHandle)
}

impl<D: Dim> World<D> {
    /// Builds a world and allocates all of its storage. This is the only call that allocates.
    ///
    /// # Errors
    /// [`PhysicsError::CapacityTooLarge`] when `max_bodies` is above [`MAX_BODIES`];
    /// [`PhysicsError::NotFinite`] when the gravity is not finite;
    /// [`PhysicsError::InvalidMargin`] when the broadphase or speculative margin is not a finite
    /// number of zero or more; [`PhysicsError::InvalidSolverConfig`] and
    /// [`PhysicsError::InvalidSleepConfig`] for a solver or sleep setting out of range; the pair and
    /// manifold capacities have their own errors.
    pub fn new(config: WorldConfig<D>) -> Result<Self, PhysicsError> {
        if config.max_bodies > MAX_BODIES {
            return Err(PhysicsError::CapacityTooLarge { max: MAX_BODIES });
        }
        if !config.gravity.is_finite() {
            return Err(PhysicsError::NotFinite);
        }
        let broadphase = Broadphase::new(
            config.max_bodies,
            config.broadphase_margin,
            config.max_pairs,
        )?;
        if !(config.speculative_margin.is_finite() && config.speculative_margin >= 0.0) {
            return Err(PhysicsError::InvalidMargin);
        }
        let manifolds = ManifoldList::new(config.max_manifolds)?;
        let joints = JointTable::new(config.max_joints)?;
        let solver = D::Solver::new(
            config.solver,
            config.max_bodies,
            config.max_manifolds,
            config.max_joints,
        )?;
        config.sleep.validate()?;
        let mut scratch = Vec::with_capacity(config.max_bodies as usize);
        scratch.resize_with(config.max_bodies as usize, || None);
        let mut slots = Vec::with_capacity(config.max_bodies as usize);
        slots.resize_with(config.max_bodies as usize, || None);
        Ok(Self {
            handles: Pool::with_capacity(config.max_bodies),
            slots,
            capacity: config.max_bodies,
            gravity: config.gravity,
            steps: 0,
            broadphase,
            manifolds,
            joints,
            speculative_margin: config.speculative_margin,
            solver,
            sleep: config.sleep,
            islands: Islands::new(config.max_bodies as usize),
            scratch,
            contacts_stale: false,
        })
    }

    /// The most bodies the world can hold.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.handles.capacity()
    }

    /// The number of live bodies.
    #[must_use]
    pub fn len(&self) -> usize {
        self.handles.len()
    }

    /// True when no body is alive.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }

    /// The acceleration of gravity.
    #[must_use]
    pub fn gravity(&self) -> D::Vec {
        self.gravity
    }

    /// The number of steps taken so far.
    #[must_use]
    pub fn step_count(&self) -> u64 {
        self.steps
    }

    /// True when `handle` names a live body of this world.
    #[must_use]
    pub fn contains(&self, handle: BodyHandle<D>) -> bool {
        live_slot(&self.slots, handle).is_ok()
    }

    /// Creates a body. Allocates nothing.
    ///
    /// # Errors
    /// The errors of the description ([`PhysicsError::NotFinite`], [`PhysicsError::InvalidShape`]),
    /// or [`PhysicsError::CapacityExhausted`] when every slot is in use. A refused call changes
    /// nothing: no slot is used up.
    pub fn create_body(&mut self, desc: &BodyDesc<D>) -> Result<BodyHandle<D>, PhysicsError> {
        let body = Body::from_desc(desc)?;
        let body_is_static = body.kind() == BodyKind::Static;
        if !self.broadphase.can_hold(body.kind(), &body.bounds()) {
            return Err(PhysicsError::NotFinite);
        }
        let handle = self
            .handles
            .insert(BodyTag::new())
            .map_err(|_| PhysicsError::CapacityExhausted)?;
        if let Err(error) = self.place(handle, body) {
            // Not reachable: the pool, the slot vector and both trees have the same capacity and
            // the box was checked above.
            self.handles.remove(handle);
            return Err(error);
        }
        self.islands.reset_slot(handle.index() as usize);
        if body_is_static {
            // A new static body may rest against, or under, a sleeping one; which, the manifolds of
            // the next step say, but a static body is not part of any island, so wake them all.
            self.wake_all();
        }
        self.contacts_stale = true;
        Ok(handle)
    }

    /// Stores `body` in the slot of `handle` and in the tree of its kind.
    fn place(&mut self, handle: BodyHandle<D>, body: Body<D>) -> Result<(), PhysicsError> {
        let slot = self
            .slots
            .get_mut(handle.index() as usize)
            .ok_or(PhysicsError::CapacityExhausted)?;
        let proxy = self
            .broadphase
            .insert(body.kind(), &body.bounds(), handle.index())?;
        *slot = Some(Slot {
            handle,
            body,
            proxy,
        });
        Ok(())
    }

    /// Destroys a body; its slot is reused by a later body with a higher generation, unless the
    /// generation cannot advance any more: such a slot is retired for good and the number of bodies
    /// that can exist at once then falls below `capacity`. The pair list keeps the pairs of the
    /// destroyed body until the next [`World::find_pairs`] or [`World::step`].
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`].
    pub fn destroy_body(&mut self, handle: BodyHandle<D>) -> Result<(), PhysicsError> {
        let slot = live_slot(&self.slots, handle)?;
        self.broadphase.remove(slot.body.kind(), slot.proxy)?;
        self.wake_around(handle);
        let (joints, slots, islands) = (&mut self.joints, &mut self.slots, &mut self.islands);
        joints.destroy_touching(handle.index(), |other| {
            wake_island_in(slots, islands, other.index() as usize);
        });
        self.handles.remove(handle);
        if let Some(slot) = self.slots.get_mut(handle.index() as usize) {
            *slot = None;
        }
        self.islands.reset_slot(handle.index() as usize);
        self.contacts_stale = true;
        Ok(())
    }

    /// Creates a joint between two live bodies. Allocates nothing. The island of both bodies wakes
    /// (a new joint changes what holds them), and unless [`JointDesc::collide_connected`] is set
    /// the two bodies make no manifold with each other from the next step on.
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`] for a body that is gone; [`PhysicsError::InvalidJoint`] when
    /// the two bodies are the same body or both static, or the shape is out of range;
    /// [`PhysicsError::NotFinite`] for a number that is not finite;
    /// [`PhysicsError::JointCapacityExhausted`] when every joint slot is in use. A refused call
    /// changes nothing.
    pub fn create_joint(&mut self, desc: &JointDesc<D>) -> Result<JointHandle<D>, PhysicsError> {
        let first = live_slot(&self.slots, desc.first)?.body.kind();
        let second = live_slot(&self.slots, desc.second)?.body.kind();
        let both_static = first == BodyKind::Static && second == BodyKind::Static;
        if desc.first == desc.second || both_static {
            return Err(PhysicsError::InvalidJoint);
        }
        let handle = self.joints.create(desc)?;
        self.rouse_dynamic(desc.first);
        self.rouse_dynamic(desc.second);
        self.contacts_stale = true;
        Ok(handle)
    }

    /// Destroys a joint; the island of its bodies wakes, since they lose a support.
    ///
    /// # Errors
    /// [`PhysicsError::StaleJoint`].
    pub fn destroy_joint(&mut self, handle: JointHandle<D>) -> Result<(), PhysicsError> {
        let entry = self.joints.destroy(handle)?;
        self.rouse_dynamic(entry.first);
        self.rouse_dynamic(entry.second);
        self.contacts_stale = true;
        Ok(())
    }

    /// True when `handle` names a live joint of this world.
    #[must_use]
    pub fn contains_joint(&self, handle: JointHandle<D>) -> bool {
        self.joints.get(handle).is_ok()
    }

    /// The number of live joints.
    #[must_use]
    pub fn joint_count(&self) -> usize {
        self.joints.len()
    }

    /// The most joints the world can hold.
    #[must_use]
    pub fn joint_capacity(&self) -> usize {
        self.joints.capacity()
    }

    /// A live joint: its bodies, shape and contact flag.
    ///
    /// # Errors
    /// [`PhysicsError::StaleJoint`].
    pub fn joint(&self, handle: JointHandle<D>) -> Result<&JointEntry<D>, PhysicsError> {
        self.joints.get(handle)
    }

    /// Every live joint, in slot-index order.
    #[must_use]
    pub fn joints(&self) -> &[JointEntry<D>] {
        self.joints.live()
    }

    /// Rouses the body of `handle` unless it is static or gone.
    fn rouse_dynamic(&mut self, handle: BodyHandle<D>) {
        let dynamic =
            live_slot(&self.slots, handle).is_ok_and(|slot| slot.body.kind() == BodyKind::Dynamic);
        if dynamic {
            self.rouse(handle.index() as usize);
        }
    }

    /// The state of a body.
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`].
    pub fn body(&self, handle: BodyHandle<D>) -> Result<&Body<D>, PhysicsError> {
        live_slot(&self.slots, handle).map(|slot| &slot.body)
    }

    /// Every live body with its handle, in slot-index order.
    pub fn iter(&self) -> impl Iterator<Item = (BodyHandle<D>, &Body<D>)> {
        self.slots
            .iter()
            .flatten()
            .map(|slot| (slot.handle, &slot.body))
    }

    /// The pairs of bodies whose broadphase boxes overlapped at the last [`World::step`] or
    /// [`World::find_pairs`], in canonical order (see [`PairList`]).
    #[must_use]
    pub fn pairs(&self) -> &PairList<D> {
        self.broadphase.pairs()
    }

    /// The manifolds of the pairs of [`World::pairs`] whose shapes touch or are within the
    /// speculative margin, as the last [`World::step`] or [`World::collide`] made them, in the order
    /// of the pair list.
    #[must_use]
    pub fn manifolds(&self) -> &ManifoldList<D> {
        &self.manifolds
    }

    /// The box the broadphase holds for a body: its tight box grown by the margin when the box was
    /// stored (it is stored again only when the body leaves it), or the tight box itself for a
    /// static body.
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`].
    pub fn broadphase_bounds(&self, handle: BodyHandle<D>) -> Result<D::Bounds, PhysicsError> {
        let slot = live_slot(&self.slots, handle)?;
        self.broadphase.bounds(slot.body.kind(), slot.proxy)
    }

    /// A hash of the structure of both trees (see [`Tree::structure_hash`]); two worlds built and
    /// stepped by the same calls have the same one. It is not part of [`World::state_hash`].
    #[must_use]
    pub fn broadphase_hash(&self) -> u64 {
        self.broadphase.structure_hash()
    }

    /// Finds the pairs again from the boxes the trees hold now. [`World::step`] does this after
    /// moving the bodies; call it after creating or destroying bodies to see their pairs without a
    /// step. Allocates nothing.
    pub fn find_pairs(&mut self) {
        self.broadphase.find_pairs(&self.slots);
    }

    /// Chooses how the contact solver executes the next steps: serially or through a scheduler,
    /// with the scalar reference or the lane path (see [`SolverExecution`]). No choice changes
    /// the state hash of any step; the default is [`SolverExecution::SERIAL`], the reference.
    pub fn set_execution(&mut self, execution: SolverExecution) {
        self.solver.set_execution(execution);
    }

    /// The solver, for tests that reorder its constraints.
    #[cfg(test)]
    pub(crate) fn solver_mut(&mut self) -> &mut D::Solver {
        &mut self.solver
    }

    /// Makes the manifolds again from the pairs the pair list holds now and the bodies as they are
    /// now. [`World::step`] does this after finding the pairs; call it after [`World::find_pairs`]
    /// to see the contacts of a scene without a step. Applies no force. Allocates nothing.
    pub fn collide(&mut self) {
        self.manifolds.clear();
        let (slots, joints, margin) = (&self.slots, &self.joints, self.speculative_margin);
        let pairs = self.broadphase.pairs().as_slice();
        for entry in pairs
            .iter()
            .filter_map(|pair| contact_of(slots, joints, pair, margin))
        {
            self.manifolds.push(entry);
        }
        self.islands
            .build(&self.slots, self.manifolds.as_slice(), self.joints.live());
        self.contacts_stale = false;
    }

    /// True when the body sleeps (see [`SleepConfig`]).
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`].
    pub fn is_asleep(&self, handle: BodyHandle<D>) -> Result<bool, PhysicsError> {
        live_slot(&self.slots, handle).map(|slot| slot.body.is_asleep())
    }

    /// Wakes the island of a dynamic body, if it sleeps, and restarts the rest timer of the body,
    /// so the island cannot fall asleep again at once. Allocates nothing.
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`], [`PhysicsError::StaticBody`].
    pub fn wake(&mut self, handle: BodyHandle<D>) -> Result<(), PhysicsError> {
        if live_slot(&self.slots, handle)?.body.kind() == BodyKind::Static {
            return Err(PhysicsError::StaticBody);
        }
        self.rouse(handle.index() as usize);
        Ok(())
    }

    /// The island of a dynamic body as of the last [`World::step`] or [`World::collide`]: the handle
    /// of the lowest-slot body of the set of dynamic bodies joined to it by manifolds, so two bodies
    /// are in one island exactly when this returns the same handle. `None` for a static body, which
    /// is in no island. A body created since then is alone in its island until the next step. After
    /// [`World::destroy_body`] of the lowest body of an island, and before the next step, the
    /// others still name that slot and the answer is the handle of the body that reused it, or
    /// their own handle when the slot is empty.
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`].
    pub fn island_of(&self, handle: BodyHandle<D>) -> Result<Option<BodyHandle<D>>, PhysicsError> {
        let slot = live_slot(&self.slots, handle)?;
        if slot.body.kind() == BodyKind::Static {
            return Ok(None);
        }
        let label = self.islands.label_of(handle.index() as usize);
        let lowest = label
            .and_then(|label| self.slots.get(label as usize))
            .and_then(Option::as_ref)
            .map_or(handle, |lowest| lowest.handle);
        Ok(Some(lowest))
    }

    /// The number of islands (a lone dynamic body is one) as of the last step or collide.
    #[must_use]
    pub fn island_count(&self) -> usize {
        self.islands.count()
    }

    /// The number of sleeping bodies.
    #[must_use]
    pub fn sleeping_count(&self) -> usize {
        self.slots
            .iter()
            .flatten()
            .filter(|slot| slot.body.is_asleep())
            .count()
    }

    /// How many seconds the body has rested (moved at or below the sleep speed limits) without
    /// interruption, as the step counted them; zero while it sleeps.
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`].
    pub fn rest_time(&self, handle: BodyHandle<D>) -> Result<f32, PhysicsError> {
        let slot = live_slot(&self.slots, handle)?;
        if slot.body.is_asleep() {
            return Ok(0.0);
        }
        Ok(self.islands.rest_of(handle.index() as usize))
    }

    /// Counters of the last solve: constraints solved and not solved, colours used, overflow
    /// constraints. They do not vary with the machine.
    #[must_use]
    pub fn solver_stats(&self) -> SolverStats {
        self.solver.stats()
    }

    /// A change by the caller reached the body in `index`: its rest timer restarts and, if it
    /// sleeps, its island wakes.
    fn rouse(&mut self, index: usize) {
        self.islands.restart(index);
        let asleep = self
            .slots
            .get(index)
            .and_then(Option::as_ref)
            .is_some_and(|slot| slot.body.is_asleep());
        if asleep {
            self.wake_island(index);
        }
    }

    /// Wakes every sleeping body in the island of the body in `index`.
    fn wake_island(&mut self, index: usize) {
        wake_island_in(&mut self.slots, &mut self.islands, index);
    }

    /// Wakes every sleeping body.
    fn wake_all(&mut self) {
        let islands = &mut self.islands;
        for (index, slot) in self
            .slots
            .iter_mut()
            .enumerate()
            .filter_map(|(index, slot)| slot.as_mut().map(|slot| (index, slot)))
            .filter(|(_, slot)| slot.body.is_asleep())
        {
            slot.body.wake_up();
            islands.restart(index);
        }
    }

    /// `handle` is about to be destroyed: the island of the body wakes, and so does the island of
    /// every body that has a manifold with it (a static body that goes away takes its support from
    /// them).
    fn wake_around(&mut self, handle: BodyHandle<D>) {
        self.wake_island(handle.index() as usize);
        for other in self
            .manifolds
            .as_slice()
            .iter()
            .filter_map(|entry| partner(entry, handle))
        {
            wake_island_in(&mut self.slots, &mut self.islands, other.index() as usize);
        }
    }

    /// Sets the linear velocity of a dynamic body. A sleeping body wakes with its island.
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`], [`PhysicsError::StaticBody`], [`PhysicsError::NotFinite`].
    pub fn set_linear_velocity(
        &mut self,
        handle: BodyHandle<D>,
        velocity: D::Vec,
    ) -> Result<(), PhysicsError> {
        live_slot_mut(&mut self.slots, handle)?
            .body
            .set_linear_velocity(velocity)?;
        self.rouse(handle.index() as usize);
        Ok(())
    }

    /// Sets the angular velocity of a dynamic body.
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`], [`PhysicsError::StaticBody`], [`PhysicsError::NotFinite`].
    pub fn set_angular_velocity(
        &mut self,
        handle: BodyHandle<D>,
        velocity: D::Ang,
    ) -> Result<(), PhysicsError> {
        live_slot_mut(&mut self.slots, handle)?
            .body
            .set_angular_velocity(velocity)?;
        self.rouse(handle.index() as usize);
        Ok(())
    }

    /// Adds a force (newtons) acting through the centre of mass during the next step only.
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`], [`PhysicsError::StaticBody`], [`PhysicsError::NotFinite`].
    pub fn apply_force(
        &mut self,
        handle: BodyHandle<D>,
        force: D::Vec,
    ) -> Result<(), PhysicsError> {
        live_slot_mut(&mut self.slots, handle)?
            .body
            .apply_force(force)?;
        self.rouse(handle.index() as usize);
        Ok(())
    }

    /// Adds a torque (newton metres) acting during the next step only.
    ///
    /// # Errors
    /// [`PhysicsError::StaleHandle`], [`PhysicsError::StaticBody`], [`PhysicsError::NotFinite`].
    pub fn apply_torque(
        &mut self,
        handle: BodyHandle<D>,
        torque: D::Ang,
    ) -> Result<(), PhysicsError> {
        live_slot_mut(&mut self.slots, handle)?
            .body
            .apply_torque(torque)?;
        self.rouse(handle.index() as usize);
        Ok(())
    }

    /// Advances the world by `dt` seconds: the contact solver moves every dynamic body (bodies in
    /// contact over sub-steps, with friction and restitution; the others by one step of semi-implicit
    /// Euler), then the boxes move in the broadphase, the pairs are found again
    /// ([`World::pairs`]) and the manifolds made again ([`World::manifolds`]) for the bodies as they
    /// are now; the next step solves those. The caller passes the same `dt` every step (the fixed
    /// step of the application); allocates nothing.
    ///
    /// Sleeping ([`SleepConfig`]): a sleeping body is not moved and is not solved, its contacts are
    /// kept as they are, and an awake body that touches it wakes its island first; after the
    /// solve, islands that have rested for the time to sleep fall asleep. The solver visits the
    /// contacts colour by colour ([`crate::core::Colouring`]).
    ///
    /// # Errors
    /// [`PhysicsError::InvalidTimeStep`] when `dt` is not finite and above zero;
    /// [`PhysicsError::NotFinite`] when the step would make a position, rotation, velocity or
    /// impulse overflow, or a broadphase box not representable. In both cases nothing changes (the
    /// solver works on copies of the bodies and the new state is committed only when all of it is
    /// valid). A pair list that is too small is not an error of the step: see
    /// [`PairList::is_complete`].
    pub fn step(&mut self, dt: f32) -> Result<(), PhysicsError> {
        if !dt.is_finite() || dt <= 0.0 {
            return Err(PhysicsError::InvalidTimeStep);
        }
        if self.contacts_stale {
            self.find_pairs();
            self.collide();
        }
        for (copy, slot) in self.scratch.iter_mut().zip(&self.slots) {
            *copy = slot.as_ref().map(|slot| slot.body);
        }
        // Islands that an awake body touches wake before the solve; on the copies, so a refused step
        // wakes nothing.
        self.islands.wake_touched(&mut self.scratch);
        let frame = StepFrame {
            gravity: self.gravity,
            dt,
        };
        self.solver.solve(
            &mut self.scratch,
            self.manifolds.as_slice(),
            self.joints.live(),
            frame,
        );
        let broadphase = &self.broadphase;
        let stays_finite = self.solver.is_finite()
            && self.scratch.iter().flatten().all(|body| {
                body.motion_is_finite() && broadphase.can_hold(body.kind(), &body.bounds())
            });
        if !stays_finite {
            return Err(PhysicsError::NotFinite);
        }
        for (slot, copy) in self.slots.iter_mut().zip(&self.scratch) {
            store(slot, copy.as_ref());
        }
        self.solver.commit();
        // `can_hold` was checked above for every body, so this cannot fail on a committed step; the
        // check and the move must stay equivalent for the step to remain atomic.
        self.broadphase.move_proxies(&self.slots)?;
        self.steps += 1;
        if self.sleep.enabled {
            self.islands.tick(&self.slots, &self.sleep, dt);
        }
        self.find_pairs();
        self.collide();
        if self.sleep.enabled {
            self.islands.sleep(&mut self.slots, &self.sleep);
        }
        Ok(())
    }

    /// A hash of the simulation state: capacity, gravity, then every live body (handle, then all
    /// state) in slot-index order, floats by bit pattern, then the warm start of the solver (the
    /// impulses of the last step's contacts; nothing when there are none). A sleeping body adds one
    /// tag byte after its state, so the sleep flag is covered and a world in which nothing sleeps
    /// adds nothing for sleeping. The rest timers are not hashed: they only decide when a body
    /// falls asleep, which the flag then shows. Equal inputs give equal hashes on every
    /// run; any change of a live body's state changes it. It does not cover the order in which freed
    /// slots will be reused, so two worlds that reached the same live bodies by different destroy
    /// orders hash equal and may hand out different handles on the next create.
    #[must_use]
    pub fn state_hash(&self) -> u64 {
        let mut hasher = StateHasher::new();
        hasher.write_u32(self.capacity);
        self.gravity.write_bits(&mut hasher);
        for slot in self.slots.iter().flatten() {
            hasher.write_u32(slot.handle.index());
            hasher.write_u32(slot.handle.generation());
            slot.body.write_bits(&mut hasher);
        }
        self.solver.write_bits(&mut hasher);
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::MAX_PAIRS;
    use crate::core::{BodyKind, Dim};
    use crate::d2::{Aabb2, D2, Rot2, Shape2};
    use base::math::Vec2;
    use base::pool::Handle;

    type World2 = World<D2>;
    type Desc2 = BodyDesc<D2>;

    const DT: f32 = 1.0 / 60.0;
    const G: f32 = -9.8;

    fn world(max_bodies: u32, gravity: Vec2) -> World2 {
        match World::new(WorldConfig {
            max_bodies,
            gravity,
            max_pairs: 256,
            broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
            max_manifolds: 256,
            max_joints: 0,
            speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
            solver: SolverConfig::DEFAULT,
            sleep: SleepConfig::DEFAULT,
        }) {
            Ok(world) => world,
            Err(error) => panic!("world must build: {error}"),
        }
    }

    fn circle(radius: f32) -> Shape2 {
        Shape2::Circle { radius }
    }

    fn dynamic(x: f32, y: f32) -> Desc2 {
        Desc2::new(BodyKind::Dynamic, Vec2::new(x, y), circle(0.5))
    }

    fn create(world: &mut World2, desc: &Desc2) -> BodyHandle<D2> {
        match world.create_body(desc) {
            Ok(handle) => handle,
            Err(error) => panic!("body must be created: {error}"),
        }
    }

    fn state(world: &World2, handle: BodyHandle<D2>) -> Body<D2> {
        match world.body(handle) {
            Ok(body) => *body,
            Err(error) => panic!("body must exist: {error}"),
        }
    }

    fn step_n(world: &mut World2, steps: u32) {
        for _ in 0..steps {
            assert_eq!(world.step(DT), Ok(()));
        }
    }

    fn stale<T>(result: Result<T, PhysicsError>) -> bool {
        result.err() == Some(PhysicsError::StaleHandle)
    }

    /// Bit-for-bit equality of two floats (the tests compare exact results on purpose).
    fn same(a: f32, b: f32) -> bool {
        a.to_bits() == b.to_bits()
    }

    // ---- creation and capacity -------------------------------------------------------------

    #[test]
    fn a_world_is_built_with_its_capacity_and_is_empty() {
        let mut world = world(8, Vec2::new(0.0, G));
        assert_eq!(
            (world.capacity(), world.len(), world.is_empty()),
            (8, 0, true)
        );
        create(&mut world, &dynamic(0.0, 0.0));
        assert_eq!((world.len(), world.is_empty()), (1, false));
        let world = self::world(8, Vec2::new(0.0, G));
        assert_eq!(world.gravity(), Vec2::new(0.0, G));
        assert_eq!(world.step_count(), 0);
    }

    #[test]
    fn a_capacity_above_the_maximum_is_refused() {
        let config = WorldConfig::<D2> {
            max_bodies: MAX_BODIES + 1,
            gravity: Vec2::ZERO,
            max_pairs: 256,
            broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
            max_manifolds: 256,
            max_joints: 0,
            speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
            solver: SolverConfig::DEFAULT,
            sleep: SleepConfig::DEFAULT,
        };
        assert!(matches!(
            World::new(config).err(),
            Some(PhysicsError::CapacityTooLarge { max }) if max == MAX_BODIES
        ));
    }

    #[test]
    fn a_gravity_that_is_not_finite_is_refused() {
        for gravity in [Vec2::new(f32::NAN, 0.0), Vec2::new(0.0, f32::INFINITY)] {
            let config = WorldConfig::<D2> {
                max_bodies: 1,
                gravity,
                max_pairs: 256,
                broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
                max_manifolds: 256,
                max_joints: 0,
                speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
                solver: SolverConfig::DEFAULT,
                sleep: SleepConfig::DEFAULT,
            };
            assert_eq!(World::new(config).err(), Some(PhysicsError::NotFinite));
        }
    }

    #[test]
    fn a_world_of_zero_capacity_refuses_every_body() {
        let mut world = world(0, Vec2::ZERO);
        assert_eq!(
            world.create_body(&dynamic(0.0, 0.0)).err(),
            Some(PhysicsError::CapacityExhausted)
        );
    }

    #[test]
    fn handles_are_issued_in_slot_order_with_generation_zero() {
        assert_eq!(std::mem::size_of::<BodyHandle<D2>>(), 8);
        let mut world = world(3, Vec2::ZERO);
        for expected in 0..3 {
            let handle = create(&mut world, &dynamic(0.0, 0.0));
            assert_eq!((handle.index(), handle.generation()), (expected, 0));
        }
        assert_eq!(world.len(), 3);
    }

    #[test]
    fn creating_past_the_capacity_is_an_error_value_and_changes_nothing() {
        let mut world = world(2, Vec2::ZERO);
        let first = create(&mut world, &dynamic(1.0, 0.0));
        create(&mut world, &dynamic(2.0, 0.0));
        let before = world.state_hash();
        assert_eq!(
            world.create_body(&dynamic(3.0, 0.0)).err(),
            Some(PhysicsError::CapacityExhausted)
        );
        assert_eq!((world.len(), world.state_hash()), (2, before));
        assert!(world.contains(first));
    }

    #[test]
    fn a_refused_description_uses_up_no_slot() {
        let mut world = world(1, Vec2::ZERO);
        let bad = Desc2::new(BodyKind::Dynamic, Vec2::ZERO, circle(-1.0));
        assert_eq!(
            world.create_body(&bad).err(),
            Some(PhysicsError::InvalidShape)
        );
        assert_eq!(world.len(), 0);
        let handle = create(&mut world, &dynamic(0.0, 0.0));
        assert_eq!((handle.index(), handle.generation()), (0, 0));
    }

    #[test]
    fn a_description_with_a_value_that_is_not_finite_is_refused() {
        let mut world = world(1, Vec2::ZERO);
        let nan = f32::NAN;
        let mut cases = [dynamic(0.0, 0.0); 6];
        cases[0].position = Vec2::new(nan, 0.0);
        cases[1].linear_velocity = Vec2::new(0.0, f32::INFINITY);
        cases[2].angular_velocity = nan;
        cases[3].rotation = Rot2::from_angle(nan);
        cases[4].linear_damping = nan;
        cases[5].angular_damping = f32::INFINITY;
        for desc in &cases {
            assert_eq!(world.create_body(desc).err(), Some(PhysicsError::NotFinite));
        }
        assert_eq!(world.len(), 0);
    }

    #[test]
    fn negative_damping_and_a_density_of_zero_are_refused() {
        let mut world = world(1, Vec2::ZERO);
        let mut damped = dynamic(0.0, 0.0);
        damped.linear_damping = -0.1;
        let mut spinning = dynamic(0.0, 0.0);
        spinning.angular_damping = -0.1;
        let mut weightless = dynamic(0.0, 0.0);
        weightless.density = 0.0;
        for desc in [damped, spinning, weightless] {
            assert_eq!(
                world.create_body(&desc).err(),
                Some(PhysicsError::InvalidShape)
            );
        }
    }

    #[test]
    fn a_shape_whose_inverse_mass_overflows_is_refused() {
        // A large box of almost no density: the mass is a denormal (inverse mass infinite) while the
        // inertia is large enough for a finite inverse inertia, so only the mass check can refuse it.
        let mut world = world(1, Vec2::ZERO);
        let mut thin = Desc2::new(
            BodyKind::Dynamic,
            Vec2::ZERO,
            Shape2::Box {
                half_extents: Vec2::new(100.0, 100.0),
            },
        );
        thin.density = 5e-44;
        assert_eq!(
            world.create_body(&thin).err(),
            Some(PhysicsError::InvalidShape)
        );
    }

    #[test]
    fn a_shape_whose_mass_and_inertia_underflow_is_refused() {
        // Mass 1e-39 is a denormal: valid, but its inverse is infinite.
        let mut world = world(1, Vec2::ZERO);
        let mut tiny = dynamic(0.0, 0.0);
        tiny.shape = circle(1e-2);
        tiny.density = 1e-36;
        assert_eq!(
            world.create_body(&tiny).err(),
            Some(PhysicsError::InvalidShape)
        );
    }

    #[test]
    fn the_body_reports_what_it_was_created_from() {
        let mut world = world(2, Vec2::ZERO);
        let mut desc = dynamic(1.0, 2.0);
        desc.rotation = Rot2::from_angle(0.5);
        desc.linear_velocity = Vec2::new(3.0, 4.0);
        desc.angular_velocity = 0.25;
        desc.density = 2.0;
        let handle = create(&mut world, &desc);
        let body = state(&world, handle);
        assert_eq!(body.kind(), BodyKind::Dynamic);
        assert_eq!(body.position(), Vec2::new(1.0, 2.0));
        assert_eq!(body.rotation(), Rot2::from_angle(0.5));
        assert_eq!(body.linear_velocity(), Vec2::new(3.0, 4.0));
        assert!(same(body.angular_velocity(), 0.25));
        assert_eq!(body.shape(), circle(0.5));
        let mass = 2.0 * std::f32::consts::PI * 0.25;
        assert!((body.mass() - mass).abs() < 1e-6);
        assert!((body.inverse_mass() * mass - 1.0).abs() < 1e-6);
        let inertia = 0.5 * mass * 0.25;
        assert!((body.inverse_inertia() * inertia - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_static_body_has_infinite_mass_and_ignores_its_velocity() {
        let mut world = world(1, Vec2::new(0.0, G));
        let mut desc = Desc2::new(BodyKind::Static, Vec2::new(1.0, 1.0), circle(1.0));
        desc.linear_velocity = Vec2::new(5.0, 5.0);
        desc.angular_velocity = 2.0;
        let handle = create(&mut world, &desc);
        let body = state(&world, handle);
        assert_eq!(body.kind(), BodyKind::Static);
        assert_eq!(
            (body.mass(), body.inverse_mass(), body.inverse_inertia()),
            (0.0, 0.0, 0.0)
        );
        assert_eq!(
            (body.linear_velocity(), body.angular_velocity()),
            (Vec2::ZERO, 0.0)
        );
    }

    #[test]
    fn a_static_body_with_an_invalid_shape_is_refused() {
        let mut world = world(1, Vec2::ZERO);
        let bad = Desc2::new(BodyKind::Static, Vec2::ZERO, circle(0.0));
        assert_eq!(
            world.create_body(&bad).err(),
            Some(PhysicsError::InvalidShape)
        );
    }

    fn cloned<T: Clone>(value: &T) -> T {
        value.clone()
    }

    #[test]
    fn descriptions_configs_and_bodies_are_plain_copyable_values() {
        let desc = dynamic(1.0, 2.0);
        let config = WorldConfig::<D2> {
            max_bodies: 3,
            gravity: Vec2::new(0.0, G),
            max_pairs: 256,
            broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
            max_manifolds: 256,
            max_joints: 0,
            speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
            solver: SolverConfig::DEFAULT,
            sleep: SleepConfig::DEFAULT,
        };
        let mut world = world(1, Vec2::ZERO);
        let handle = create(&mut world, &cloned(&desc));
        let body = state(&world, handle);
        let copy = cloned(&body);
        assert_eq!(copy.position(), body.position());
        assert_eq!(cloned(&config).max_bodies, 3);
        let Ok(properties) = D2::mass_properties(&circle(1.0), 1.0) else {
            panic!("a unit circle has mass properties");
        };
        assert!(same(cloned(&properties).mass, properties.mass));
    }

    // ---- destruction, stale handles, slot reuse --------------------------------------------

    #[test]
    fn a_destroyed_body_is_gone_and_its_handle_is_stale() {
        let mut world = world(2, Vec2::ZERO);
        let handle = create(&mut world, &dynamic(0.0, 0.0));
        assert_eq!(world.destroy_body(handle), Ok(()));
        assert_eq!((world.len(), world.contains(handle)), (0, false));
        assert!(stale(world.body(handle)));
        assert!(stale(world.destroy_body(handle)));
        assert!(stale(world.set_linear_velocity(handle, Vec2::ZERO)));
        assert!(stale(world.set_angular_velocity(handle, 0.0)));
        assert!(stale(world.apply_force(handle, Vec2::ZERO)));
        assert!(stale(world.apply_torque(handle, 0.0)));
    }

    #[test]
    fn a_reused_slot_gets_the_next_generation_and_the_old_handle_stays_stale() {
        let mut world = world(1, Vec2::ZERO);
        let old = create(&mut world, &dynamic(1.0, 0.0));
        assert_eq!(world.destroy_body(old), Ok(()));
        let new = create(&mut world, &dynamic(2.0, 0.0));
        assert_eq!((new.index(), new.generation()), (0, 1));
        assert!(stale(world.body(old)));
        assert_eq!(state(&world, new).position(), Vec2::new(2.0, 0.0));
        assert!(!world.contains(old));
        assert!(world.contains(new));
    }

    #[test]
    fn a_forged_handle_is_stale_not_a_panic() {
        let mut world = world(2, Vec2::ZERO);
        let live = create(&mut world, &dynamic(0.0, 0.0));
        let far = Handle::from_raw(1_000_000, 0);
        let wrong_generation = Handle::from_raw(live.index(), live.generation() + 1);
        let never_used = Handle::from_raw(1, 0);
        for handle in [far, wrong_generation, never_used] {
            assert!(stale(world.body(handle)));
            assert!(stale(world.destroy_body(handle)));
        }
        assert!(world.contains(live));
    }

    #[test]
    fn the_free_slot_destroyed_last_is_reused_first() {
        let mut world = world(3, Vec2::ZERO);
        let handles: Vec<_> = (0..3_u8)
            .map(|i| create(&mut world, &dynamic(f32::from(i), 0.0)))
            .collect();
        assert_eq!(world.destroy_body(handles[0]), Ok(()));
        assert_eq!(world.destroy_body(handles[2]), Ok(()));
        assert_eq!(create(&mut world, &dynamic(9.0, 0.0)).index(), 2);
        assert_eq!(create(&mut world, &dynamic(9.0, 0.0)).index(), 0);
    }

    #[test]
    fn iteration_is_in_slot_order_and_skips_destroyed_bodies() {
        let mut world = world(4, Vec2::ZERO);
        let handles: Vec<_> = (0..4_u8)
            .map(|i| create(&mut world, &dynamic(f32::from(i), 0.0)))
            .collect();
        assert_eq!(world.destroy_body(handles[1]), Ok(()));
        let seen: Vec<(u32, f32)> = world
            .iter()
            .map(|(handle, body)| (handle.index(), body.position().x))
            .collect();
        assert_eq!(seen, vec![(0, 0.0), (2, 2.0), (3, 3.0)]);
    }

    // ---- motion setters ---------------------------------------------------------------------

    #[test]
    fn velocities_can_be_set_on_a_dynamic_body() {
        let mut world = world(1, Vec2::ZERO);
        let handle = create(&mut world, &dynamic(0.0, 0.0));
        assert_eq!(
            world.set_linear_velocity(handle, Vec2::new(1.0, 2.0)),
            Ok(())
        );
        assert_eq!(world.set_angular_velocity(handle, 3.0), Ok(()));
        let body = state(&world, handle);
        assert_eq!(
            (body.linear_velocity(), body.angular_velocity()),
            (Vec2::new(1.0, 2.0), 3.0)
        );
    }

    #[test]
    fn a_static_body_takes_no_motion() {
        let mut world = world(1, Vec2::ZERO);
        let handle = create(
            &mut world,
            &Desc2::new(BodyKind::Static, Vec2::ZERO, circle(1.0)),
        );
        let before = world.state_hash();
        let one = Vec2::new(1.0, 1.0);
        assert_eq!(
            world.set_linear_velocity(handle, one),
            Err(PhysicsError::StaticBody)
        );
        assert_eq!(
            world.set_angular_velocity(handle, 1.0),
            Err(PhysicsError::StaticBody)
        );
        assert_eq!(
            world.apply_force(handle, one),
            Err(PhysicsError::StaticBody)
        );
        assert_eq!(
            world.apply_torque(handle, 1.0),
            Err(PhysicsError::StaticBody)
        );
        assert_eq!(world.state_hash(), before);
    }

    #[test]
    fn values_that_are_not_finite_are_refused_and_change_nothing() {
        let mut world = world(1, Vec2::ZERO);
        let handle = create(&mut world, &dynamic(0.0, 0.0));
        let before = world.state_hash();
        let bad = Vec2::new(f32::NAN, 0.0);
        assert_eq!(
            world.set_linear_velocity(handle, bad),
            Err(PhysicsError::NotFinite)
        );
        assert_eq!(
            world.set_angular_velocity(handle, f32::NAN),
            Err(PhysicsError::NotFinite)
        );
        assert_eq!(world.apply_force(handle, bad), Err(PhysicsError::NotFinite));
        assert_eq!(
            world.apply_torque(handle, f32::INFINITY),
            Err(PhysicsError::NotFinite)
        );
        assert_eq!(world.state_hash(), before);
    }

    // ---- the step ---------------------------------------------------------------------------

    #[test]
    fn a_time_step_that_is_not_finite_and_above_zero_is_refused_and_changes_nothing() {
        let mut world = world(1, Vec2::new(0.0, G));
        create(&mut world, &dynamic(0.0, 0.0));
        let before = world.state_hash();
        for dt in [0.0, -DT, f32::NAN, f32::INFINITY] {
            assert_eq!(
                world.step(dt),
                Err(PhysicsError::InvalidTimeStep),
                "dt {dt}"
            );
        }
        assert_eq!((world.state_hash(), world.step_count()), (before, 0));
    }

    #[test]
    fn free_fall_follows_the_semi_implicit_euler_closed_form() {
        // v_n = n g dt, y_n = y_0 + g dt^2 n (n + 1) / 2.
        let mut world = world(1, Vec2::new(0.0, G));
        let handle = create(&mut world, &dynamic(0.0, 100.0));
        let steps = 120_u16;
        step_n(&mut world, u32::from(steps));
        let n = f32::from(steps);
        let body = state(&world, handle);
        let velocity = n * G * DT;
        let height = 100.0 + G * DT * DT * n * (n + 1.0) / 2.0;
        assert!((body.linear_velocity().y - velocity).abs() < 1e-3 * velocity.abs());
        assert!((body.position().y - height).abs() < 1e-4 * 100.0);
        assert!(same(body.linear_velocity().x, 0.0));
        assert!(same(body.position().x, 0.0));
        assert_eq!(world.step_count(), 120);
    }

    #[test]
    fn free_fall_stays_within_the_documented_error_of_the_continuous_solution() {
        // The scheme differs from y_0 + g t^2 / 2 by g t dt / 2 (first order in dt).
        for steps in [1_u16, 10, 60, 600] {
            let mut world = world(1, Vec2::new(0.0, G));
            let handle = create(&mut world, &dynamic(0.0, 0.0));
            step_n(&mut world, u32::from(steps));
            let time = f32::from(steps) * DT;
            let exact = 0.5 * G * time * time;
            let bound = 0.5 * G.abs() * time * DT;
            let error = (state(&world, handle).position().y - exact).abs();
            assert!(
                error <= bound * 1.01 + 1e-3,
                "t {time}: error {error} bound {bound}"
            );
        }
    }

    #[test]
    fn gravity_does_not_depend_on_mass() {
        let mut world = world(2, Vec2::new(0.0, G));
        let light = create(&mut world, &dynamic(0.0, 0.0));
        let mut heavy_desc = dynamic(1.0, 0.0);
        heavy_desc.density = 1000.0;
        let heavy = create(&mut world, &heavy_desc);
        step_n(&mut world, 30);
        assert_eq!(
            state(&world, light).position().y.to_bits(),
            state(&world, heavy).position().y.to_bits()
        );
    }

    #[test]
    fn a_static_body_does_not_fall() {
        let mut world = world(1, Vec2::new(0.0, G));
        let handle = create(
            &mut world,
            &Desc2::new(BodyKind::Static, Vec2::new(1.0, 2.0), circle(1.0)),
        );
        let before = world.state_hash();
        step_n(&mut world, 10);
        assert_eq!(state(&world, handle).position(), Vec2::new(1.0, 2.0));
        assert_eq!(world.state_hash(), before);
    }

    #[test]
    fn momentum_is_conserved_without_forces() {
        let mut world = world(2, Vec2::ZERO);
        let mut a = dynamic(0.0, 0.0);
        a.linear_velocity = Vec2::new(3.0, -1.0);
        a.density = 2.0;
        let mut b = dynamic(5.0, 0.0);
        b.linear_velocity = Vec2::new(-1.0, 0.5);
        let (ha, hb) = (create(&mut world, &a), create(&mut world, &b));
        let momentum = |world: &World2| {
            let (pa, pb) = (state(world, ha), state(world, hb));
            pa.linear_velocity() * pa.mass() + pb.linear_velocity() * pb.mass()
        };
        let before = momentum(&world);
        step_n(&mut world, 1000);
        let after = momentum(&world);
        assert_eq!(
            (before.x.to_bits(), before.y.to_bits()),
            (after.x.to_bits(), after.y.to_bits())
        );
        let body = state(&world, ha);
        assert_eq!(body.linear_velocity(), Vec2::new(3.0, -1.0));
        let expected = 3.0 * DT * 1000.0;
        assert!(
            (body.position().x - expected).abs() < 1e-2,
            "x {}",
            body.position().x
        );
    }

    #[test]
    fn a_force_acts_for_one_step_only_and_changes_momentum_by_force_times_time() {
        let mut world = world(1, Vec2::ZERO);
        let handle = create(&mut world, &dynamic(0.0, 0.0));
        let mass = state(&world, handle).mass();
        assert_eq!(world.apply_force(handle, Vec2::new(10.0, 0.0)), Ok(()));
        assert_eq!(world.apply_force(handle, Vec2::new(2.0, 0.0)), Ok(()));
        step_n(&mut world, 1);
        let velocity = state(&world, handle).linear_velocity().x;
        assert!(
            (velocity * mass - 12.0 * DT).abs() < 1e-5,
            "p {}",
            velocity * mass
        );
        step_n(&mut world, 5);
        assert!(same(state(&world, handle).linear_velocity().x, velocity));
    }

    #[test]
    fn a_torque_acts_for_one_step_only_and_spins_by_torque_over_inertia() {
        let mut world = world(1, Vec2::ZERO);
        let handle = create(&mut world, &dynamic(0.0, 0.0));
        let inv_inertia = state(&world, handle).inverse_inertia();
        assert_eq!(world.apply_torque(handle, 4.0), Ok(()));
        step_n(&mut world, 1);
        let spin = state(&world, handle).angular_velocity();
        assert!((spin - inv_inertia * 4.0 * DT).abs() < 1e-6);
        step_n(&mut world, 5);
        assert!(same(state(&world, handle).angular_velocity(), spin));
    }

    #[test]
    fn damping_scales_the_velocity_by_one_over_one_plus_dt_times_damping() {
        let mut world = world(1, Vec2::ZERO);
        let mut desc = dynamic(0.0, 0.0);
        desc.linear_velocity = Vec2::new(10.0, 0.0);
        desc.angular_velocity = 4.0;
        desc.linear_damping = 2.0;
        desc.angular_damping = 5.0;
        let handle = create(&mut world, &desc);
        step_n(&mut world, 1);
        let body = state(&world, handle);
        let linear = 10.0 / (1.0 + DT * 2.0);
        let angular = 4.0 / (1.0 + DT * 5.0);
        assert!((body.linear_velocity().x - linear).abs() < 1e-5);
        assert!((body.angular_velocity() - angular).abs() < 1e-5);
    }

    #[test]
    fn a_spinning_body_keeps_its_rotation_pair_normalised_and_turns_at_the_expected_rate() {
        let mut world = world(1, Vec2::ZERO);
        let mut desc = dynamic(0.0, 0.0);
        desc.angular_velocity = 3.0;
        let handle = create(&mut world, &desc);
        step_n(&mut world, 10_000);
        let (rot_cos, rot_sin) = state(&world, handle).rotation().direction();
        let length = rot_cos * rot_cos + rot_sin * rot_sin;
        assert!((length - 1.0).abs() < 1e-6, "length^2 {length}");
        // Each step multiplies by (1 + i w dt) and normalises; the same in f64, with no trig.
        let delta = f64::from(3.0_f32 * DT);
        let (mut cos, mut sin) = (1.0_f64, 0.0_f64);
        for _ in 0..10_000 {
            let (c, s) = (cos - sin * delta, sin + cos * delta);
            let length = (c * c + s * s).sqrt();
            (cos, sin) = (c / length, s / length);
        }
        assert!((f64::from(rot_cos) - cos).abs() < 1e-3);
        assert!((f64::from(rot_sin) - sin).abs() < 1e-3);
    }

    #[test]
    fn a_step_that_would_overflow_is_refused_and_changes_nothing() {
        let mut world = world(2, Vec2::ZERO);
        let calm = create(&mut world, &dynamic(0.0, 0.0));
        let mut fast = dynamic(1.0, 0.0);
        fast.angular_velocity = 1e24;
        let fast = create(&mut world, &fast);
        let before = world.state_hash();
        assert_eq!(world.step(DT), Err(PhysicsError::NotFinite));
        assert_eq!(world.state_hash(), before);
        assert_eq!(world.step_count(), 0);
        assert!(same(state(&world, calm).position().y, 0.0));
        assert!(world.set_angular_velocity(fast, 1.0).is_ok());
        assert_eq!(world.step(DT), Ok(()));
        let mut huge = dynamic(f32::MAX, 0.0);
        huge.linear_velocity = Vec2::new(f32::MAX, 0.0);
        let mut world = self::world(1, Vec2::ZERO);
        create(&mut world, &huge);
        assert_eq!(world.step(1.0), Err(PhysicsError::NotFinite));
    }

    #[test]
    fn forces_and_torques_whose_sum_overflows_are_refused() {
        let mut world = world(1, Vec2::ZERO);
        let handle = create(&mut world, &dynamic(0.0, 0.0));
        assert_eq!(world.apply_force(handle, Vec2::new(f32::MAX, 0.0)), Ok(()));
        let before = world.state_hash();
        assert_eq!(
            world.apply_force(handle, Vec2::new(f32::MAX, 0.0)),
            Err(PhysicsError::NotFinite)
        );
        assert_eq!(world.state_hash(), before);
        assert_eq!(world.apply_torque(handle, f32::MAX), Ok(()));
        let before = world.state_hash();
        assert_eq!(
            world.apply_torque(handle, f32::MAX),
            Err(PhysicsError::NotFinite)
        );
        assert_eq!(world.state_hash(), before);
    }

    // ---- the state hash ---------------------------------------------------------------------

    /// A small scene built the same way every time.
    fn scene() -> World2 {
        let mut world = world(8, Vec2::new(0.0, G));
        create(
            &mut world,
            &Desc2::new(
                BodyKind::Static,
                Vec2::ZERO,
                Shape2::Box {
                    half_extents: Vec2::new(10.0, 0.5),
                },
            ),
        );
        for i in 0..5_u8 {
            let mut desc = dynamic(f32::from(i), 3.0 + f32::from(i));
            desc.angular_velocity = 0.5 * f32::from(i);
            desc.linear_velocity = Vec2::new(1.0, 0.0);
            create(&mut world, &desc);
        }
        world
    }

    #[test]
    fn the_same_inputs_give_the_same_hash_after_every_step() {
        let (mut first, mut second) = (scene(), scene());
        assert_eq!(first.state_hash(), second.state_hash());
        for _ in 0..200 {
            assert_eq!(first.step(DT), Ok(()));
            assert_eq!(second.step(DT), Ok(()));
            assert_eq!(first.state_hash(), second.state_hash());
        }
    }

    #[test]
    fn stepping_changes_the_hash_and_the_hash_is_stable_between_calls() {
        let mut world = scene();
        let before = world.state_hash();
        assert_eq!(before, world.state_hash());
        step_n(&mut world, 1);
        assert_ne!(world.state_hash(), before);
    }

    #[test]
    fn changing_one_field_of_one_body_changes_the_hash() {
        let baseline = {
            let mut world = world(2, Vec2::ZERO);
            create(&mut world, &dynamic(0.0, 0.0));
            world.state_hash()
        };
        let mut variants: Vec<Desc2> = Vec::new();
        let mut push = |change: &dyn Fn(&mut Desc2)| {
            let mut desc = dynamic(0.0, 0.0);
            change(&mut desc);
            variants.push(desc);
        };
        push(&|d| d.kind = BodyKind::Static);
        push(&|d| d.position = Vec2::new(f32::from_bits(0.0_f32.to_bits() | 1), 0.0));
        push(&|d| d.position = Vec2::new(0.0, 1e-30));
        push(&|d| d.rotation = Rot2::from_angle(1e-3));
        push(&|d| d.linear_velocity = Vec2::new(1e-30, 0.0));
        push(&|d| d.linear_velocity = Vec2::new(0.0, 1e-30));
        push(&|d| d.angular_velocity = 1e-30);
        push(&|d| d.shape = circle(0.25));
        push(&|d| {
            d.shape = Shape2::Box {
                half_extents: Vec2::new(0.5, 0.5),
            }
        });
        push(&|d| d.density = 2.0);
        push(&|d| d.linear_damping = 0.1);
        push(&|d| d.angular_damping = 0.1);
        for (i, desc) in variants.iter().enumerate() {
            let mut world = world(2, Vec2::ZERO);
            create(&mut world, desc);
            assert_ne!(world.state_hash(), baseline, "variant {i}");
        }
    }

    #[test]
    fn pending_force_and_torque_and_the_world_settings_are_part_of_the_hash() {
        let build = |capacity: u32, gravity: Vec2| {
            let mut world = world(capacity, gravity);
            let handle = create(&mut world, &dynamic(0.0, 0.0));
            (world, handle)
        };
        let (base_world, _) = build(2, Vec2::ZERO);
        let baseline = base_world.state_hash();
        let (mut forced, handle) = build(2, Vec2::ZERO);
        assert_eq!(forced.apply_force(handle, Vec2::new(0.0, 1e-30)), Ok(()));
        assert_ne!(forced.state_hash(), baseline);
        let (mut torqued, handle) = build(2, Vec2::ZERO);
        assert_eq!(torqued.apply_torque(handle, 1e-30), Ok(()));
        assert_ne!(torqued.state_hash(), baseline);
        assert_ne!(build(3, Vec2::ZERO).0.state_hash(), baseline);
        assert_ne!(build(2, Vec2::new(0.0, 1e-30)).0.state_hash(), baseline);
    }

    #[test]
    fn the_handle_and_the_set_of_live_bodies_are_part_of_the_hash() {
        let mut world = world(2, Vec2::ZERO);
        let first = create(&mut world, &dynamic(0.0, 0.0));
        let with_one = world.state_hash();
        let second = create(&mut world, &dynamic(1.0, 0.0));
        let with_two = world.state_hash();
        assert_ne!(with_one, with_two);
        assert_eq!(world.destroy_body(second), Ok(()));
        assert_eq!(world.state_hash(), with_one);
        // The same body in the same slot with a newer generation is a different state.
        assert_eq!(world.destroy_body(first), Ok(()));
        create(&mut world, &dynamic(0.0, 0.0));
        assert_ne!(world.state_hash(), with_one);
    }

    #[test]
    fn two_bodies_swapped_in_slot_order_hash_differently() {
        let build = |a: f32, b: f32| {
            let mut world = world(2, Vec2::ZERO);
            create(&mut world, &dynamic(a, 0.0));
            create(&mut world, &dynamic(b, 0.0));
            world.state_hash()
        };
        assert_ne!(build(1.0, 2.0), build(2.0, 1.0));
    }

    // ---- broadphase and pairs ---------------------------------------------------------------

    fn broadphase_world(max_bodies: u32, max_pairs: u32, margin: f32) -> World2 {
        match World::new(WorldConfig {
            max_bodies,
            gravity: Vec2::ZERO,
            max_pairs,
            broadphase_margin: margin,
            max_manifolds: 256,
            max_joints: 0,
            speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
            solver: SolverConfig::DEFAULT,
            sleep: SleepConfig::DEFAULT,
        }) {
            Ok(world) => world,
            Err(error) => panic!("world must build: {error}"),
        }
    }

    fn body_at(world: &mut World2, kind: BodyKind, x: f32, radius: f32) -> BodyHandle<D2> {
        create(world, &Desc2::new(kind, Vec2::new(x, 0.0), circle(radius)))
    }

    /// The pair list as slot-index pairs.
    fn pair_indices(world: &World2) -> Vec<(u32, u32)> {
        world
            .pairs()
            .as_slice()
            .iter()
            .map(|pair| (pair.first.index(), pair.second.index()))
            .collect()
    }

    fn found(world: &mut World2) -> Vec<(u32, u32)> {
        world.find_pairs();
        pair_indices(world)
    }

    #[test]
    fn a_manifold_capacity_above_the_maximum_or_a_bad_speculative_margin_refuses_the_world() {
        let config = |max_manifolds, speculative_margin| WorldConfig::<D2> {
            max_bodies: 4,
            gravity: Vec2::ZERO,
            max_pairs: 4,
            broadphase_margin: 0.1,
            max_manifolds,
            max_joints: 0,
            speculative_margin,
            solver: SolverConfig::DEFAULT,
            sleep: SleepConfig::DEFAULT,
        };
        assert_eq!(
            World::new(config(MAX_PAIRS + 1, 0.02)).err(),
            Some(PhysicsError::ManifoldCapacityTooLarge { max: MAX_PAIRS })
        );
        for margin in [-1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(
                World::new(config(4, margin)).err(),
                Some(PhysicsError::InvalidMargin)
            );
        }
        assert!(World::new(config(MAX_PAIRS, 0.0)).is_ok());
        let world = World::new(config(7, DEFAULT_SPECULATIVE_MARGIN));
        assert_eq!(world.map(|w| w.manifolds().capacity()).ok(), Some(7));
    }

    #[test]
    fn a_step_makes_the_manifolds_of_the_pairs_and_collide_makes_them_again() {
        let mut world = world(4, Vec2::ZERO);
        let first = create(&mut world, &dynamic(0.0, 0.0));
        let second = create(&mut world, &dynamic(0.9, 0.0));
        create(&mut world, &dynamic(5.0, 0.0));
        assert!(world.manifolds().is_empty());
        step_n(&mut world, 1);
        let [entry] = world.manifolds().as_slice() else {
            panic!("one manifold expected");
        };
        assert_eq!((entry.first, entry.second), (first, second));
        let before = world.manifolds().hash();
        world.collide();
        assert_eq!(
            (world.manifolds().len(), world.manifolds().hash()),
            (1, before)
        );
        // Moving a body far away and finding the pairs again leaves nothing to collide.
        assert!(world.contains(second));
        assert_eq!(world.destroy_body(second), Ok(()));
        world.collide();
        assert!(world.manifolds().is_empty());
    }

    #[test]
    fn a_pair_capacity_above_the_maximum_or_a_bad_margin_refuses_the_world() {
        let config = |max_pairs, broadphase_margin| WorldConfig::<D2> {
            max_bodies: 4,
            gravity: Vec2::ZERO,
            max_pairs,
            broadphase_margin,
            max_manifolds: 256,
            max_joints: 0,
            speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
            solver: SolverConfig::DEFAULT,
            sleep: SleepConfig::DEFAULT,
        };
        assert_eq!(
            World::new(config(MAX_PAIRS + 1, 0.1)).err(),
            Some(PhysicsError::PairCapacityTooLarge { max: MAX_PAIRS })
        );
        for margin in [-1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(
                World::new(config(4, margin)).err(),
                Some(PhysicsError::InvalidMargin)
            );
        }
        assert!(World::new(config(MAX_PAIRS, 0.0)).is_ok());
    }

    #[test]
    fn a_new_world_has_an_empty_complete_pair_list_of_the_configured_capacity() {
        let world = broadphase_world(4, 7, 0.1);
        assert_eq!(world.pairs().capacity(), 7);
        assert!(world.pairs().is_empty() && world.pairs().is_complete());
    }

    #[test]
    fn the_broadphase_box_of_a_dynamic_body_is_grown_by_the_margin_and_a_static_one_is_not() {
        let mut world = broadphase_world(4, 4, 0.5);
        let moving = body_at(&mut world, BodyKind::Dynamic, 2.0, 1.0);
        let fixed = body_at(&mut world, BodyKind::Static, 2.0, 1.0);
        let aabb =
            |x0: f32, y0: f32, x1: f32, y1: f32| Aabb2::new(Vec2::new(x0, y0), Vec2::new(x1, y1));
        assert_eq!(
            world.broadphase_bounds(moving),
            Ok(aabb(0.5, -1.5, 3.5, 1.5))
        );
        assert_eq!(
            world.broadphase_bounds(fixed),
            Ok(aabb(1.0, -1.0, 3.0, 1.0))
        );
        assert_eq!(world.destroy_body(moving), Ok(()));
        assert_eq!(
            world.broadphase_bounds(moving),
            Err(PhysicsError::StaleHandle)
        );
    }

    #[test]
    fn pairs_appear_only_after_find_pairs_or_a_step() {
        let mut world = broadphase_world(4, 4, 0.0);
        body_at(&mut world, BodyKind::Dynamic, 0.0, 1.0);
        body_at(&mut world, BodyKind::Dynamic, 1.0, 1.0);
        assert!(world.pairs().is_empty());
        assert_eq!(found(&mut world), vec![(0, 1)]);
        assert_eq!(world.step(DT), Ok(()));
        assert_eq!(pair_indices(&world), vec![(0, 1)]);
    }

    #[test]
    fn boxes_that_touch_exactly_pair_and_boxes_with_a_gap_do_not() {
        let mut world = broadphase_world(4, 4, 0.0);
        body_at(&mut world, BodyKind::Dynamic, 0.0, 0.5);
        body_at(&mut world, BodyKind::Dynamic, 1.0, 0.5); // touches the first at x = 0.5
        body_at(&mut world, BodyKind::Dynamic, 2.25, 0.5); // gap of 0.25 to the second
        assert_eq!(found(&mut world), vec![(0, 1)]);
    }

    #[test]
    fn a_margin_that_overflows_a_box_refuses_the_body_or_the_step_and_changes_nothing() {
        let mut world = broadphase_world(2, 4, f32::MAX);
        let far = BodyDesc::new(
            BodyKind::Dynamic,
            Vec2::new(3.0e38, 0.0),
            Shape2::Circle { radius: 1.0 },
        );
        assert_eq!(world.create_body(&far).err(), Some(PhysicsError::NotFinite));
        assert!(world.is_empty());
        let mut runner = BodyDesc::new(
            BodyKind::Dynamic,
            Vec2::ZERO,
            Shape2::Circle { radius: 1.0 },
        );
        runner.linear_velocity = Vec2::new(3.0e38, 0.0);
        assert!(world.create_body(&runner).is_ok());
        let (hash, tree, steps) = (
            world.state_hash(),
            world.broadphase_hash(),
            world.step_count(),
        );
        assert_eq!(world.step(1.0), Err(PhysicsError::NotFinite));
        assert_eq!(
            (
                world.state_hash(),
                world.broadphase_hash(),
                world.step_count()
            ),
            (hash, tree, steps)
        );
        // The slot was not used up by the refused body: the next one still gets generation 0.
        let again = world.create_body(&BodyDesc::new(
            BodyKind::Static,
            Vec2::ZERO,
            Shape2::Circle { radius: 1.0 },
        ));
        assert_eq!(again.map(base::pool::Handle::generation), Ok(0));
    }

    #[test]
    fn the_margin_makes_pairs_of_bodies_that_do_not_touch_yet() {
        let mut world = broadphase_world(4, 4, 0.5);
        body_at(&mut world, BodyKind::Dynamic, 0.0, 0.5);
        body_at(&mut world, BodyKind::Dynamic, 1.5, 0.5); // a gap of 0.5, but the grown boxes overlap
        body_at(&mut world, BodyKind::Dynamic, 4.0, 0.5); // a gap of 1.5 to the second, boxes apart
        assert_eq!(found(&mut world), vec![(0, 1)]);
    }

    #[test]
    fn two_static_bodies_never_pair_and_a_static_and_a_dynamic_body_do() {
        let mut world = broadphase_world(8, 8, 0.0);
        body_at(&mut world, BodyKind::Static, 0.0, 1.0); // slot 0
        body_at(&mut world, BodyKind::Static, 1.0, 1.0); // slot 1
        body_at(&mut world, BodyKind::Dynamic, 0.5, 1.0); // slot 2
        assert_eq!(found(&mut world), vec![(0, 2), (1, 2)]);
    }

    #[test]
    fn every_dynamic_pair_is_listed_once_with_the_lower_slot_first() {
        let mut world = broadphase_world(8, 8, 0.0);
        for _ in 0..4 {
            body_at(&mut world, BodyKind::Dynamic, 0.0, 1.0);
        }
        assert_eq!(
            found(&mut world),
            vec![(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)]
        );
        for pair in world.pairs().as_slice() {
            assert!(pair.first.index() < pair.second.index());
        }
    }

    #[test]
    fn a_step_updates_the_pairs_of_bodies_that_moved() {
        let mut world = broadphase_world(4, 4, 0.0);
        let runner = body_at(&mut world, BodyKind::Dynamic, 0.0, 0.5);
        // Above the runner's path, with the boxes overlapping but not the circles: the solver
        // leaves the runner alone.
        create(
            &mut world,
            &Desc2::new(BodyKind::Dynamic, Vec2::new(5.0, 0.9), circle(0.5)),
        );
        assert_eq!(
            world.set_linear_velocity(runner, Vec2::new(4.0, 0.0)),
            Ok(())
        );
        assert_eq!(world.step(1.0), Ok(()));
        // x = 4: the runner's box reaches 4.5, where the other one starts.
        assert_eq!(pair_indices(&world), vec![(0, 1)]);
        assert_eq!(world.step(1.0), Ok(()));
        // x = 8: the runner has passed.
        assert!(pair_indices(&world).is_empty());
    }

    #[test]
    fn destroying_a_body_removes_its_pairs_at_the_next_find_and_its_slot_pairs_again_when_reused() {
        let mut world = broadphase_world(4, 4, 0.0);
        body_at(&mut world, BodyKind::Dynamic, 0.0, 1.0);
        let middle = body_at(&mut world, BodyKind::Dynamic, 1.0, 1.0);
        assert_eq!(found(&mut world), vec![(0, 1)]);
        assert_eq!(world.destroy_body(middle), Ok(()));
        assert_eq!(
            pair_indices(&world),
            vec![(0, 1)],
            "stale until the next find"
        );
        assert!(found(&mut world).is_empty());
        let again = body_at(&mut world, BodyKind::Dynamic, 1.0, 1.0);
        assert_eq!((again.index(), again.generation()), (1, 1));
        assert_eq!(found(&mut world), vec![(0, 1)]);
        assert_eq!(world.pairs().as_slice()[0].second, again);
    }

    #[test]
    fn a_full_pair_list_counts_the_pairs_it_could_not_take_and_the_step_still_succeeds() {
        let mut world = broadphase_world(8, 2, 0.0);
        for _ in 0..4 {
            body_at(&mut world, BodyKind::Dynamic, 0.0, 1.0);
        }
        assert_eq!(world.step(DT), Ok(()));
        let pairs = world.pairs();
        assert_eq!(
            (pairs.len(), pairs.missing(), pairs.is_complete()),
            (2, 4, false)
        );
        let mut roomy = broadphase_world(8, 6, 0.0);
        for _ in 0..4 {
            body_at(&mut roomy, BodyKind::Dynamic, 0.0, 1.0);
        }
        roomy.find_pairs();
        assert!(roomy.pairs().is_complete());
        assert_eq!(roomy.pairs().len(), 6);
    }

    /// A world of five bodies that move up at different speeds.
    fn moving_world() -> World2 {
        let mut world = broadphase_world(8, 8, 0.1);
        for i in 0..5_u8 {
            moving_body(&mut world, i);
        }
        world
    }

    /// A dynamic body at x = 0.8 i that moves up with speed i.
    fn moving_body(world: &mut World2, i: u8) {
        let body = body_at(world, BodyKind::Dynamic, f32::from(i) * 0.8, 0.5);
        assert_eq!(
            world.set_linear_velocity(body, Vec2::new(0.0, f32::from(i))),
            Ok(())
        );
    }

    #[test]
    fn the_broadphase_hash_is_the_same_for_the_same_calls_and_moves_with_the_bodies() {
        let (mut first, mut second) = (moving_world(), moving_world());
        assert_eq!(first.broadphase_hash(), second.broadphase_hash());
        for _ in 0..10 {
            assert_eq!(first.step(DT), Ok(()));
            assert_eq!(second.step(DT), Ok(()));
            assert_eq!(first.broadphase_hash(), second.broadphase_hash());
            assert_eq!(pair_indices(&first), pair_indices(&second));
        }
        let before = first.broadphase_hash();
        assert_eq!(first.step(1.0), Ok(()));
        assert_ne!(first.broadphase_hash(), before);
    }
}
