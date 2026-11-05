//! Islands and sleeping (card 4-14).
//!
//! An *island* is a set of dynamic bodies joined by touching manifolds: bodies that can influence
//! each other this step. A static body never joins islands, so two piles resting on the same ground
//! are two islands. The islands are found by a union-find over the manifold list; the root of a set
//! is always its lowest slot index, so an island's id (its *label*) is the slot index of its lowest
//! body, whatever the order the manifolds come in. Nothing here allocates after [`Islands::new`], and
//! nothing depends on an address, a hash or the thread count.
//!
//! *Sleeping.* Every awake dynamic body keeps a rest timer: it grows by `dt` after a step in which
//! the body moved at or below the speed limits ([`SleepConfig`]) and is zero otherwise. An island
//! sleeps when every one of its bodies is awake and the smallest rest timer is at least the time to
//! sleep. A sleeping body is not moved by the step and has zero velocity. The island wakes when an
//! awake body touches it (a manifold between them exists), or by the explicit calls of the world.
//!
//! The island of a body is only valid for the manifolds it was built from: [`Islands::build`] runs
//! whenever the world makes its manifolds.

use super::body::{Body, BodyKind};
use super::cells::{change, index_u32, put, read};
use super::dim::Dim;
use super::manifolds::ContactManifold;
use super::sleep::SleepConfig;
use super::world::{Slot, live_slot};

/// The islands of a world and the rest timers of its bodies.
pub(super) struct Islands {
    /// Union-find forest by slot index. After [`Islands::build`] every entry is the root, which is
    /// the lowest slot of the island.
    label: Vec<u32>,
    /// Seconds each body has rested (see the module comment); meaningful for awake dynamic bodies.
    rest: Vec<f32>,
    /// Scratch of the wake pass: true at a label whose island holds an awake body.
    awake: Vec<bool>,
    /// Scratch of the sleep pass: the smallest rest timer of the island at a label.
    smallest: Vec<f32>,
    /// The number of islands found by the last build.
    sets: usize,
}

impl Islands {
    /// Storage for `max_bodies` bodies.
    #[must_use]
    pub(super) fn new(max_bodies: usize) -> Self {
        Self {
            label: (0..max_bodies).map(index_u32).collect(),
            rest: vec![0.0; max_bodies],
            awake: vec![false; max_bodies],
            smallest: vec![0.0; max_bodies],
            sets: 0,
        }
    }

    /// The number of islands (sets of dynamic bodies, counting a lone body) found by the last
    /// [`Islands::build`].
    #[must_use]
    pub(super) fn count(&self) -> usize {
        self.sets
    }

    /// The label (lowest slot index) of the island of `slot`; the slot itself if it joined none.
    #[must_use]
    pub(super) fn label_of(&self, slot: usize) -> Option<u32> {
        self.label.get(slot).copied()
    }

    /// A body of `slot` was created or destroyed: it is alone in its island until the next build,
    /// and its rest timer starts at zero.
    pub(super) fn reset_slot(&mut self, slot: usize) {
        put(&mut self.label, slot, index_u32(slot));
        self.restart(slot);
    }

    /// Sets the rest timer of `slot` to zero.
    pub(super) fn restart(&mut self, slot: usize) {
        put(&mut self.rest, slot, 0.0);
    }

    /// The seconds the body in `slot` has rested.
    #[must_use]
    pub(super) fn rest_of(&self, slot: usize) -> f32 {
        read(&self.rest, slot, 0.0)
    }

    /// Finds the islands of `manifolds`: a union of the two bodies of every manifold whose bodies
    /// are both dynamic.
    pub(super) fn build<D: Dim>(
        &mut self,
        slots: &[Option<Slot<D>>],
        manifolds: &[ContactManifold<D>],
    ) {
        for (index, label) in self.label.iter_mut().enumerate() {
            *label = index_u32(index);
        }
        for entry in manifolds.iter().filter(|entry| joins(slots, entry)) {
            unite(&mut self.label, entry.first.index(), entry.second.index());
        }
        flatten(&mut self.label);
        self.sets = (0..self.label.len())
            .filter(|&index| {
                is_dynamic(slots, index) && self.label_of(index) == Some(index_u32(index))
            })
            .count();
    }

    /// Wakes, in `bodies` (the copies the solver is about to move), every sleeping body whose
    /// island holds an awake body: awake bodies and sleeping bodies that touch each other are one
    /// island, and an island is awake or asleep as a whole. The rest timers of the woken bodies
    /// restart (also in a step that is refused afterwards: a sleeper's timer is not observable).
    pub(super) fn wake_touched<D: Dim>(&mut self, bodies: &mut [Option<Body<D>>]) {
        self.awake.fill(false);
        for (index, _) in bodies
            .iter()
            .enumerate()
            .filter(|(_, body)| body.as_ref().is_some_and(is_awake_dynamic))
        {
            self.mark(index);
        }
        for (index, body) in bodies
            .iter_mut()
            .enumerate()
            .filter_map(|(index, body)| body.as_mut().map(|body| (index, body)))
        {
            self.rouse(index, body);
        }
    }

    /// Wakes `body` (in `index`) if its island holds an awake body.
    fn rouse<D: Dim>(&mut self, index: usize, body: &mut Body<D>) {
        if body.is_asleep() && self.is_marked(index) {
            body.wake_up();
            self.restart(index);
        }
    }

    /// Marks the island of `slot` as holding an awake body.
    fn mark(&mut self, slot: usize) {
        let label = read(&self.label, slot, 0);
        put(&mut self.awake, label as usize, true);
    }

    fn is_marked(&self, slot: usize) -> bool {
        let label = read(&self.label, slot, 0);
        read(&self.awake, label as usize, false)
    }

    /// After a step: every awake dynamic body that moved at or below the speed limits has rested
    /// `dt` seconds longer, every other one starts again from zero.
    pub(super) fn tick<D: Dim>(
        &mut self,
        slots: &[Option<Slot<D>>],
        config: &SleepConfig,
        dt: f32,
    ) {
        for (index, body) in dynamic_bodies(slots).filter(|(_, body)| !body.is_asleep()) {
            let resting = body.rests_below(config.linear_speed, config.angular_speed);
            let rest = rested(self.rest_of(index), resting, dt);
            put(&mut self.rest, index, rest);
        }
    }

    /// Puts to sleep every island whose bodies are all awake and have all rested for
    /// `config.time_to_sleep`.
    pub(super) fn sleep<D: Dim>(&mut self, slots: &mut [Option<Slot<D>>], config: &SleepConfig) {
        self.smallest.fill(f32::INFINITY);
        for (index, body) in dynamic_bodies(slots) {
            // A sleeping body holds its island back: an island that is part asleep has not rested.
            let rest = self.rest_if_awake(index, body.is_asleep());
            self.lower(index, rest);
        }
        for (index, slot) in slots
            .iter_mut()
            .enumerate()
            .filter_map(|(index, slot)| slot.as_mut().map(|slot| (index, slot)))
        {
            self.fall_asleep(index, &mut slot.body, config);
        }
    }

    /// Puts `body` (in `index`) to sleep if it is an awake dynamic body of an island that has
    /// rested long enough.
    fn fall_asleep<D: Dim>(&self, index: usize, body: &mut Body<D>, config: &SleepConfig) {
        let sleeps = body.kind() == BodyKind::Dynamic
            && !body.is_asleep()
            && self.island_rest(index) >= config.time_to_sleep;
        if sleeps {
            body.fall_asleep();
        }
    }

    /// The rest timer of `slot`, or zero for a sleeping body.
    fn rest_if_awake(&self, slot: usize, asleep: bool) -> f32 {
        if asleep { 0.0 } else { self.rest_of(slot) }
    }

    /// Lowers the smallest rest timer of the island of `slot` to `rested` if it is lower.
    fn lower(&mut self, slot: usize, rested: f32) {
        let label = read(&self.label, slot, 0);
        change(&mut self.smallest, label as usize, |smallest| {
            *smallest = smallest.min(rested);
        });
    }

    fn island_rest(&self, slot: usize) -> f32 {
        let label = read(&self.label, slot, 0);
        read(&self.smallest, label as usize, 0.0)
    }
}

/// The rest timer after one step of `dt`.
fn rested(rest: f32, resting: bool, dt: f32) -> f32 {
    if resting { rest + dt } else { 0.0 }
}

/// True for an awake dynamic body.
fn is_awake_dynamic<D: Dim>(body: &Body<D>) -> bool {
    body.kind() == BodyKind::Dynamic && !body.is_asleep()
}

/// The dynamic bodies of `slots` with their slot index, in slot order.
fn dynamic_bodies<D: Dim>(slots: &[Option<Slot<D>>]) -> impl Iterator<Item = (usize, &Body<D>)> {
    slots
        .iter()
        .enumerate()
        .filter_map(|(index, slot)| slot.as_ref().map(|slot| (index, &slot.body)))
        .filter(|(_, body)| body.kind() == BodyKind::Dynamic)
}

/// Whether the body in `index` is a live dynamic body.
fn is_dynamic<D: Dim>(slots: &[Option<Slot<D>>], index: usize) -> bool {
    slots
        .get(index)
        .and_then(Option::as_ref)
        .is_some_and(|slot| slot.body.kind() == BodyKind::Dynamic)
}

/// Whether both bodies of `entry` are live dynamic bodies, which is when the manifold joins them.
fn joins<D: Dim>(slots: &[Option<Slot<D>>], entry: &ContactManifold<D>) -> bool {
    [entry.first, entry.second].iter().all(|&handle| {
        live_slot(slots, handle).is_ok_and(|slot| slot.body.kind() == BodyKind::Dynamic)
    })
}

/// The root of the set of `slot`, shortening the path on the way (halving).
fn find(label: &mut [u32], slot: u32) -> u32 {
    let mut current = slot;
    loop {
        let parent = read(label, current as usize, current);
        if parent == current {
            return current;
        }
        let grand = read(label, parent as usize, parent);
        put(label, current as usize, grand);
        current = grand;
    }
}

/// Joins the sets of two slots; the lower root becomes the root of both.
fn unite(label: &mut [u32], a: u32, b: u32) {
    let (a, b) = (find(label, a), find(label, b));
    let (low, high) = if a < b { (a, b) } else { (b, a) };
    put(label, high as usize, low);
}

/// Points every slot straight at its root.
fn flatten(label: &mut [u32]) {
    for index in 0..label.len() {
        let root = find(label, index_u32(index));
        put(label, index, root);
    }
}
