//! The staged colour solve (card 4-15): the constraints of one colour are cut into fixed chunks,
//! the chunks run through a scheduler (or on the calling thread), and the bodies are written back
//! serially afterwards.
//!
//! Why it is data-race free in safe Rust and equal to the serial reference:
//!
//! * the constraints of a colour touch pairwise different moving bodies (`core::colouring`), so
//!   what one constraint computes does not depend on what another computes in the same pass;
//! * a chunk only **reads** the shared body array (`&[SolverBody]`) and works on copies; the one
//!   thing it writes is the slot of each of its own constraints, behind a `Mutex` that no other
//!   chunk locks (a member appears in exactly one chunk), so the lock is never contended;
//! * the scheduler's join is the barrier between colours: the next colour starts after all chunks
//!   returned, and sees the bodies written back by the serial scatter;
//! * the scatter stores the copies of the dynamic bodies in the canonical member order, which is
//!   the order the reference stores them in, and a static body is never stored (as in the
//!   reference).
//!
//! The path assumes what the world guarantees before a solve: a constraint end that the colouring
//! does not protect (a body that is not awake and dynamic) is never written back, because
//! `World::step` wakes every sleeper an awake body touches, so such an end is static.
//!
//! The cut into chunks depends on the size of the colour and the chunk size only. How a scheduler
//! orders, groups or places the chunks cannot matter, because no chunk reads what another writes.

use super::constraint::{LANES, Pass, SolverBody, Staged, run_lanes};
use base::fpenv::FpGuard;
use jobs::scheduler::IScheduler;
use std::sync::{Mutex, PoisonError};

#[cfg(test)]
thread_local! {
    /// Groups of four that took the lane path on this thread (tests only: the lane path gives the
    /// same bits as the scalar one, so only a counter shows that it ran).
    pub(super) static LANE_GROUPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A constraint of the step with its working copies, behind the lock a chunk takes to load and to
/// store it (each member of a group is locked twice per pass, by one chunk only).
pub(super) struct Slot(Mutex<Staged>);

impl Slot {
    pub(super) fn new(staged: &Staged) -> Self {
        Self(Mutex::new(*staged))
    }

    /// The contents (a copy). A poisoned lock is read as it is: a panic aborts the process, so no
    /// reader can see the state of an unwound writer.
    pub(super) fn load(&self) -> Staged {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn store(&self, staged: &Staged) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = *staged;
    }

    /// The slot indices of the two bodies of the constraint (for the tests).
    #[cfg(test)]
    pub(super) fn ends(&self) -> [usize; 2] {
        let constraint = self.load().constraint;
        [constraint.a, constraint.b]
    }

    /// Direct access when the slot is not shared (no lock is taken).
    pub(super) fn get_mut(&mut self) -> &mut Staged {
        self.0.get_mut().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What every chunk of one pass over one colour shares.
pub(super) struct ColourJob<'a> {
    pub slots: &'a [Slot],
    pub bodies: &'a [SolverBody],
    /// The constraints of the colour, in canonical order.
    pub members: &'a [u32],
    pub pass: Pass<'a>,
    pub simd: bool,
    /// Constraints per chunk, at least 1.
    pub chunk: usize,
}

impl ColourJob<'_> {
    /// The number of chunks the colour is cut into.
    pub(super) fn chunk_count(&self) -> usize {
        self.members.len().div_ceil(self.chunk.max(1))
    }

    /// Runs chunk `index`: the members `index * chunk ..` up to the chunk size. The thread's
    /// floating-point control state is put to the deterministic default for the chunk and restored
    /// after it (`base::fpenv`; a target without the helper reports a value, which is ignored: it
    /// has nothing to set). The function is the opaque boundary the helper asks for: the guard is
    /// taken here and the arithmetic runs in [`Self::run_group`], which is never inlined.
    pub(super) fn run_chunk(&self, index: usize) {
        let _guard = FpGuard::deterministic();
        let size = self.chunk.max(1);
        let from = index.saturating_mul(size);
        let to = from.saturating_add(size).min(self.members.len());
        let Some(part) = self.members.get(from..to) else {
            return;
        };
        for group in part.chunks(LANES) {
            self.run_group(group);
        }
    }

    /// Loads, runs and stores up to [`LANES`] members.
    #[inline(never)]
    fn run_group(&self, group: &[u32]) {
        let loaded: [Option<Staged>; LANES] =
            std::array::from_fn(|lane| group.get(lane).and_then(|&member| self.load(member)));
        let done = self.compute(&loaded);
        for (&member, staged) in group.iter().zip(&done) {
            self.put(member, staged.as_ref());
        }
    }

    /// Runs the pass on the loaded constraints: four at once with lanes when the lane path is on
    /// and all four are there, one by one otherwise (the scalar reference).
    fn compute(&self, loaded: &[Option<Staged>; LANES]) -> [Option<Staged>; LANES] {
        if let ([Some(a), Some(b), Some(c), Some(d)], true) = (loaded, self.simd) {
            let mut four = [*a, *b, *c, *d];
            #[cfg(test)]
            LANE_GROUPS.with(|count| count.set(count.get() + 1));
            run_lanes(&mut four, self.pass);
            return four.map(Some);
        }
        loaded.map(|staged| staged.map(|staged| self.run_scalar(staged)))
    }

    fn run_scalar(&self, mut staged: Staged) -> Staged {
        staged.run(self.pass);
        staged
    }

    /// Stores the result of `member` into its slot.
    fn put(&self, member: u32, staged: Option<&Staged>) {
        let slot = self.slots.get(member as usize);
        slot.zip(staged)
            .into_iter()
            .for_each(|(slot, staged)| slot.store(staged));
    }

    /// The slot of `member` with the current copies of its two bodies; `None` when a body is
    /// missing (the reference skips such a constraint too).
    fn load(&self, member: u32) -> Option<Staged> {
        let mut staged = self.slots.get(member as usize)?.load();
        staged.a = *self.bodies.get(staged.constraint.a)?;
        staged.b = *self.bodies.get(staged.constraint.b)?;
        Some(staged)
    }

    /// Runs every chunk on the calling thread, in order.
    fn run_inline(&self) {
        (0..self.chunk_count()).for_each(|index| self.run_chunk(index));
    }

    /// Runs the colour: through `scheduler` when there is one and the colour has more than one
    /// chunk, otherwise (or when the scheduler refuses the call, in which case it ran nothing) on
    /// the calling thread.
    pub(super) fn run(&self, scheduler: Option<&dyn IScheduler>) {
        let chunks = self.chunk_count();
        let Some(scheduler) = scheduler.filter(|_| chunks > 1) else {
            return self.run_inline();
        };
        let Ok(count) = u32::try_from(chunks) else {
            return self.run_inline();
        };
        let ran = scheduler.parallel_for(count, 1, &|range| {
            range.for_each(|index| self.run_chunk(index as usize));
        });
        // `IScheduler` runs no chunk when it returns an error, so running them all here once is
        // right (a chunk is not idempotent: the impulses accumulate).
        if ran.is_err() {
            self.run_inline();
        }
    }
}

/// Stores the copies of the dynamic bodies of `members` into `bodies`, in canonical order. Every
/// member was loaded from valid body slots by the job, so the copies are never placeholders.
pub(super) fn scatter(slots: &mut [Slot], bodies: &mut [SolverBody], members: &[u32]) {
    for &member in members {
        let Some(slot) = slots.get_mut(member as usize) else {
            continue;
        };
        let staged = *slot.get_mut();
        store_body(bodies, staged.constraint.a, staged.a);
        store_body(bodies, staged.constraint.b, staged.b);
    }
}

/// Stores `body` at `index` unless it is a static body (which no constraint changes).
fn store_body(bodies: &mut [SolverBody], index: usize, body: SolverBody) {
    if !body.is_static() {
        bodies
            .get_mut(index)
            .into_iter()
            .for_each(|target| *target = body);
    }
}
