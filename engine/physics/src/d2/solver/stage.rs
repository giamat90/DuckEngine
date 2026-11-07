//! The staged colour solve (card 4-15, reworked in card 4-30): the constraints of one colour are cut
//! into fixed chunks, the chunks run through a scheduler (or on the calling thread), and the
//! results are written back serially afterwards.
//!
//! Why it is data-race free in safe Rust and equal to the serial reference:
//!
//! * the constraints of a colour touch pairwise different moving bodies (`core::colouring`), so
//!   what one constraint computes does not depend on what another computes in the same pass;
//! * a chunk only **reads** the shared constraint and body arrays and works on copies it builds in
//!   its own output buffer; the one thing it writes is that buffer, behind a `Mutex` that no
//!   other chunk locks (a chunk has its own buffer) and that it locks once per chunk, so the lock
//!   is never contended and costs nothing per constraint;
//! * the scheduler's join is the barrier between colours: the next colour starts after all chunks
//!   returned, and sees the constraints and bodies written back by the serial scatter;
//! * the scatter stores the buffers in chunk order, which is the canonical member order, and it
//!   stores the copies of the dynamic bodies in the order the reference stores them in, a static
//!   body never (as in the reference).
//!
//! Card 4-30 replaced a lock and several copies of the whole record per constraint and pass (a
//! `Mutex<Staged>` per constraint) by one lock per chunk and two copies (into the buffer, back in
//! the scatter): the staged path took 2.7 times the time of the whole serial solve loop on one
//! thread (`reviews/4-30.md`).
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
use std::sync::{Mutex, MutexGuard, PoisonError};

#[cfg(test)]
thread_local! {
    /// Groups of four that took the lane path on this thread (tests only: the lane path gives the
    /// same bits as the scalar one, so only a counter shows that it ran).
    pub(super) static LANE_GROUPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// What one chunk of a pass produced: the constraints it ran with the copies of their bodies, and
/// the index of each (a constraint with a missing body is skipped, as the reference skips it).
pub(super) struct ChunkOut {
    members: Vec<u32>,
    staged: Vec<Staged>,
}

impl ChunkOut {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            members: Vec::with_capacity(capacity),
            staged: Vec::with_capacity(capacity),
        }
    }
}

/// The output buffers for a solver of at most `max_constraints` constraints cut into chunks of
/// `chunk` (at least 1): one per chunk of the largest possible colour, each with room for a whole
/// chunk, so a pass allocates nothing.
pub(super) fn make_outputs(max_constraints: usize, chunk: usize) -> Vec<Mutex<ChunkOut>> {
    let chunk = chunk.max(1);
    (0..max_constraints.div_ceil(chunk))
        .map(|_| Mutex::new(ChunkOut::with_capacity(chunk)))
        .collect()
}

/// Locks a buffer; a poisoned lock is read as it is: a panic aborts the process, so no reader can
/// see the state of an unwound writer.
fn lock(out: &Mutex<ChunkOut>) -> MutexGuard<'_, ChunkOut> {
    out.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What every chunk of one pass over one colour shares.
pub(super) struct ColourJob<'a> {
    pub constraints: &'a [Staged],
    pub bodies: &'a [SolverBody],
    /// The constraints of the colour, in canonical order.
    pub members: &'a [u32],
    pub pass: Pass<'a>,
    pub simd: bool,
    /// Constraints per chunk, at least 1.
    pub chunk: usize,
    /// One buffer per chunk (at least [`Self::chunk_count`] of them).
    pub outputs: &'a [Mutex<ChunkOut>],
}

impl ColourJob<'_> {
    /// The number of chunks the colour is cut into.
    pub(super) fn chunk_count(&self) -> usize {
        self.members.len().div_ceil(self.chunk.max(1))
    }

    /// True when the buffers are enough for the chunks (always, for a solver that sized them with
    /// [`make_outputs`] for its chunk size; if not, the caller runs the reference).
    pub(super) fn has_room(&self) -> bool {
        self.outputs.len() >= self.chunk_count()
    }

    /// Runs chunk `index`: the members `index * chunk ..` up to the chunk size. The thread's
    /// floating-point control state is put to the deterministic default for the chunk and restored
    /// after it (`base::fpenv`; a target without the helper reports a value, which is ignored: it
    /// has nothing to set). The function is the opaque boundary the helper asks for: the guard is
    /// taken here and the arithmetic runs in [`Self::compute`], which is never inlined.
    pub(super) fn run_chunk(&self, index: usize) {
        let _guard = FpGuard::deterministic();
        let size = self.chunk.max(1);
        let from = index.saturating_mul(size);
        let to = from.saturating_add(size).min(self.members.len());
        let (Some(part), Some(out)) = (self.members.get(from..to), self.outputs.get(index)) else {
            return;
        };
        let mut out = lock(out);
        out.members.clear();
        out.staged.clear();
        for &member in part {
            self.load(member, &mut out);
        }
        self.compute(&mut out.staged);
    }

    /// Appends `member` with the current copies of its two bodies; nothing when a body is missing.
    fn load(&self, member: u32, out: &mut ChunkOut) {
        let Some(source) = self.constraints.get(member as usize) else {
            return;
        };
        let ends = (
            self.bodies.get(source.constraint.a),
            self.bodies.get(source.constraint.b),
        );
        if let (Some(&a), Some(&b)) = ends {
            out.members.push(member);
            out.staged.push(Staged {
                constraint: source.constraint,
                a,
                b,
            });
        }
    }

    /// Runs the pass on the loaded constraints: four at once with lanes when the lane path is on
    /// and all four are there, one by one otherwise (the scalar reference).
    #[inline(never)]
    fn compute(&self, staged: &mut [Staged]) {
        if self.simd {
            staged
                .chunks_mut(LANES)
                .for_each(|group| self.run_group(group));
        } else {
            self.run_each(staged);
        }
    }

    /// One group of the lane path: lanes when it holds four, the scalar reference otherwise.
    fn run_group(&self, group: &mut [Staged]) {
        match <&mut [Staged; LANES]>::try_from(&mut *group) {
            Ok(four) => self.run_four(four),
            Err(_) => self.run_each(group),
        }
    }

    fn run_four(&self, four: &mut [Staged; LANES]) {
        #[cfg(test)]
        LANE_GROUPS.with(|count| count.set(count.get() + 1));
        run_lanes(four, self.pass);
    }

    /// The scalar reference, one constraint after the other.
    fn run_each(&self, staged: &mut [Staged]) {
        for one in staged {
            one.run(self.pass);
        }
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

/// Stores the results of the first `chunks` buffers into `constraints` and `bodies`, in canonical
/// order. Every constraint in a buffer was loaded from valid body slots, so the copies are never
/// placeholders.
pub(super) fn scatter(
    outputs: &mut [Mutex<ChunkOut>],
    chunks: usize,
    constraints: &mut [Staged],
    bodies: &mut [SolverBody],
) {
    for out in outputs.iter_mut().take(chunks) {
        let out = out.get_mut().unwrap_or_else(PoisonError::into_inner);
        for (&member, staged) in out.members.iter().zip(&out.staged) {
            store_result(constraints, bodies, member, staged);
        }
    }
}

/// Stores one result: the constraint with its new impulses and the dynamic bodies it moved.
fn store_result(
    constraints: &mut [Staged],
    bodies: &mut [SolverBody],
    member: u32,
    staged: &Staged,
) {
    store_body(bodies, staged.constraint.a, staged.a);
    store_body(bodies, staged.constraint.b, staged.b);
    constraints
        .get_mut(member as usize)
        .into_iter()
        .for_each(|target| *target = *staged);
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
