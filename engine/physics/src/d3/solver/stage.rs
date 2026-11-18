//! The staged colour solve (card 8-19, the 3D twin of `d2::solver::stage`): the constraints of one
//! colour are cut into fixed chunks, the chunks run through a scheduler (or on the calling
//! thread), and the results are written back serially afterwards.
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
//! The path assumes what the world guarantees before a solve: a constraint end that the colouring
//! does not protect (a body that is not awake and dynamic) is never written back, because
//! `World::step` wakes every sleeper an awake body touches, so such an end is static.
//!
//! The cut into chunks depends on the size of the colour and the chunk size only. How a scheduler
//! orders, groups or places the chunks cannot matter, because no chunk reads what another writes.
//! There is no lane (SIMD) path in 3D: a chunk runs the scalar solve one constraint at a time.

use super::body::SolverBody;
use super::constraint::{Constraint, Pass, Staged};
use base::fpenv::FpGuard;
use jobs::scheduler::IScheduler;
use std::sync::{Mutex, MutexGuard, PoisonError};

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
/// chunk (at most `max_constraints`), so a pass allocates nothing.
pub(super) fn make_outputs(max_constraints: usize, chunk: usize) -> Vec<Mutex<ChunkOut>> {
    let chunk = chunk.max(1);
    // A buffer never holds more than the solver has constraints, whatever the chunk size asks.
    let room = chunk.min(max_constraints);
    (0..max_constraints.div_ceil(chunk))
        .map(|_| Mutex::new(ChunkOut::with_capacity(room)))
        .collect()
}

/// Locks a buffer; a poisoned lock is read as it is: a panic aborts the process, so no reader can
/// see the state of an unwound writer.
fn lock(out: &Mutex<ChunkOut>) -> MutexGuard<'_, ChunkOut> {
    out.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What every chunk of one pass over one colour shares.
pub(super) struct ColourJob<'a> {
    pub constraints: &'a [Constraint],
    pub bodies: &'a [SolverBody],
    /// The constraints of the colour, in canonical order.
    pub members: &'a [u32],
    pub pass: Pass<'a>,
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
    /// after it (`base::fpenv`). The function is the opaque boundary the helper asks for: the guard
    /// is taken here and the arithmetic runs in [`Self::compute`], which is never inlined.
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
        self.compute(&mut out);
    }

    /// Appends `member` with the current copies of its two bodies; nothing when a body is missing.
    fn load(&self, member: u32, out: &mut ChunkOut) {
        let Some(source) = self.constraints.get(member as usize) else {
            return;
        };
        let ends = (
            self.bodies.get(source.shape.a),
            self.bodies.get(source.shape.b),
        );
        if let (Some(&a), Some(&b)) = ends {
            out.members.push(member);
            out.staged.push(Staged {
                impulses: source.impulses,
                a,
                b,
            });
        }
    }

    /// Runs the pass on the loaded constraints, one after the other. `members` and `staged` are
    /// the lists of a [`ChunkOut`] and as long as each other.
    #[inline(never)]
    fn compute(&self, out: &mut ChunkOut) {
        for (&member, one) in out.members.iter().zip(&mut out.staged) {
            self.run_one(member, one);
        }
    }

    /// Runs the pass on the copies of constraint `member`.
    fn run_one(&self, member: u32, one: &mut Staged) {
        if let Some(source) = self.constraints.get(member as usize) {
            one.run(&source.shape, self.pass);
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
        // right (a chunk clears its own buffer first, so running it again is harmless anyway).
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
    constraints: &mut [Constraint],
    bodies: &mut [SolverBody],
) {
    for out in outputs.iter_mut().take(chunks) {
        let out = out.get_mut().unwrap_or_else(PoisonError::into_inner);
        for (&member, staged) in out.members.iter().zip(&out.staged) {
            store_result(constraints, bodies, member, staged);
        }
    }
}

/// Stores one result: the new impulses of the constraint and the dynamic bodies it moved.
fn store_result(
    constraints: &mut [Constraint],
    bodies: &mut [SolverBody],
    member: u32,
    staged: &Staged,
) {
    let Some(target) = constraints.get_mut(member as usize) else {
        return;
    };
    store_body(bodies, target.shape.a, staged.a);
    store_body(bodies, target.shape.b, staged.b);
    target.impulses = staged.impulses;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_buffer_never_holds_more_than_the_solver_has_constraints() {
        // A chunk size far above the capacity must not size the buffers by the chunk size.
        let outputs = make_outputs(10, 100_000);
        assert_eq!(outputs.len(), 1);
        for out in outputs {
            let out = out.into_inner().unwrap_or_else(PoisonError::into_inner);
            assert!(out.members.capacity() <= 10, "{}", out.members.capacity());
            assert!(out.staged.capacity() <= 10, "{}", out.staged.capacity());
        }
    }

    #[test]
    fn there_is_one_buffer_per_chunk_of_the_largest_colour_and_a_chunk_of_zero_is_one() {
        assert_eq!(make_outputs(10, 4).len(), 3);
        assert_eq!(make_outputs(10, 5).len(), 2);
        assert_eq!(make_outputs(10, 0).len(), 10);
        assert_eq!(make_outputs(0, 4).len(), 0);
    }
}
