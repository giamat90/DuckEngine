//! The making of the constraints of a step through a scheduler (card 8-112): the manifolds that make
//! a constraint are cut into fixed chunks, each chunk builds its constraints into a buffer of its
//! own, and the buffers are stored into the solver one after the other, in chunk order, which is
//! the order of the manifold list.
//!
//! Why it equals the serial loop bit for bit: building the constraint of one manifold reads the
//! manifold, the two bodies of the world and the two solver bodies (which the serial touching of the
//! ends has finished by then) and writes nothing shared, so what a chunk makes depends on its
//! manifolds only; the cut depends on the number of manifolds and the chunk size, never on the
//! workers. What stays on one thread is what is ordered: the touching of the bodies (the order in
//! which they are first met), the colouring list and the warm start, whose cursor walks the list.

use super::constraint::Constraint;
use crate::core::SolverExecution;
use base::fpenv::FpGuard;
use jobs::scheduler::IScheduler;
use std::sync::{Arc, Mutex, PoisonError};

/// The fewest chunks of manifolds worth a hand-over to the scheduler.
const MIN_CHUNKS: usize = 2;

/// Chunks per scheduler job: each chunk is one index of the loop.
const INDICES_PER_JOB: u32 = 1;

/// The constraint a manifold of the list makes: `None` for an index that makes none.
pub(super) type MakerOf<'a> = dyn Fn(u32) -> Option<Constraint> + Sync + 'a;

/// A constraint with the index of the manifold it was made from.
pub(super) type Made = (u32, Constraint);

/// The scheduler and the buffers (one per chunk of the largest possible list) of the constraints;
/// built when the execution has a scheduler, so a step allocates nothing.
pub(super) struct PrepareRun {
    scheduler: Arc<dyn IScheduler>,
    chunk: usize,
    buffers: Vec<Mutex<Vec<Made>>>,
}

impl PrepareRun {
    /// The run for `execution` and at most `max_constraints` constraints; `None` without a
    /// scheduler (the constraints are then made on the calling thread).
    pub(super) fn new(execution: &SolverExecution, max_constraints: usize) -> Option<Self> {
        let scheduler = Arc::clone(execution.scheduler.as_ref()?);
        let chunk = execution.chunk_size();
        let room = chunk.min(max_constraints);
        let buffers = (0..max_constraints.div_ceil(chunk))
            .map(|_| Mutex::new(Vec::with_capacity(room)))
            .collect();
        Some(Self {
            scheduler,
            chunk,
            buffers,
        })
    }

    /// Makes the constraint of each manifold of `members` into the buffers. Returns the number of
    /// chunks used, or `None`, having made nothing, when the members make a single chunk or more
    /// chunks than there are buffers (the caller then makes them itself).
    pub(super) fn run(&self, members: &[u32], make: &MakerOf<'_>) -> Option<usize> {
        let chunks = members.len().div_ceil(self.chunk);
        let count = u32::try_from(chunks).ok()?;
        if chunks < MIN_CHUNKS || chunks > self.buffers.len() {
            return None;
        }
        let ran = self
            .scheduler
            .parallel_for(count, INDICES_PER_JOB, &|range| {
                range.for_each(|index| self.make_chunk(index as usize, members, make));
            });
        // A refusing scheduler ran nothing, so the chunks run here (a chunk clears its own buffer
        // first, so running one twice is harmless).
        if ran.is_err() {
            (0..chunks).for_each(|index| self.make_chunk(index, members, make));
        }
        Some(chunks)
    }

    /// Calls `store` with each constraint of the first `chunks` buffers, in chunk order.
    pub(super) fn drain(&self, chunks: usize, mut store: impl FnMut(Made)) {
        for buffer in self.buffers.iter().take(chunks) {
            let buffer = buffer.lock().unwrap_or_else(PoisonError::into_inner);
            buffer.iter().for_each(|&made| store(made));
        }
    }

    /// Makes the constraints of chunk `index` into its buffer (under the deterministic
    /// floating-point state, as every simulation job is).
    fn make_chunk(&self, index: usize, members: &[u32], make: &MakerOf<'_>) {
        let (Some(buffer), Some(part)) = (
            self.buffers.get(index),
            members.chunks(self.chunk).nth(index),
        ) else {
            return;
        };
        let _guard = FpGuard::deterministic();
        let mut buffer = buffer.lock().unwrap_or_else(PoisonError::into_inner);
        buffer.clear();
        buffer.extend(
            part.iter()
                .filter_map(|&manifold| make(manifold).map(|made| (manifold, made))),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jobs::inline::InlineScheduler;

    fn execution(chunk: u32) -> SolverExecution {
        SolverExecution {
            scheduler: Some(Arc::new(InlineScheduler)),
            simd: false,
            chunk,
        }
    }

    #[test]
    fn there_is_no_run_without_a_scheduler() {
        assert!(PrepareRun::new(&SolverExecution::SERIAL, 100).is_none());
    }

    #[test]
    fn one_buffer_is_made_for_each_chunk_of_the_largest_list_and_none_is_made_for_an_empty_one() {
        assert_eq!(
            PrepareRun::new(&execution(32), 100).map(|run| run.buffers.len()),
            Some(4)
        );
        assert_eq!(
            PrepareRun::new(&execution(32), 64).map(|run| run.buffers.len()),
            Some(2)
        );
        assert_eq!(
            PrepareRun::new(&execution(32), 0).map(|run| run.buffers.len()),
            Some(0)
        );
    }

    #[test]
    fn a_buffer_is_never_larger_than_the_whole_list() {
        let run = PrepareRun::new(&execution(32), 5);
        assert_eq!(run.as_ref().map(|run| run.buffers.len()), Some(1));
        let room = run.and_then(|run| {
            run.buffers
                .first()
                .map(|buffer| buffer.lock().map(|b| b.capacity()).unwrap_or(0))
        });
        assert_eq!(room, Some(5));
    }

    #[test]
    fn a_single_chunk_or_more_chunks_than_buffers_is_declined_and_makes_nothing() {
        let Some(run) = PrepareRun::new(&execution(4), 8) else {
            panic!("a run with a scheduler");
        };
        let nothing: &MakerOf<'_> = &|_| None;
        assert_eq!(run.run(&[0, 1, 2, 3], nothing), None, "one chunk");
        let members: Vec<u32> = (0..12).collect();
        assert_eq!(
            run.run(&members, nothing),
            None,
            "three chunks, two buffers"
        );
        assert_eq!(run.run(&members[..8], nothing), Some(2));
    }
}
