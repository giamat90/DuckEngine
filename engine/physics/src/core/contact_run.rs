//! The contact tests of the pair list through a scheduler (card 8-104): the pairs are cut into
//! fixed chunks, each chunk tests its pairs into a buffer of its own, and the buffers are stored
//! into the manifold list serially, in chunk order, which is the order of the pair list.
//!
//! Why it equals the serial loop bit for bit: the test of one pair reads the bodies and the joint
//! table and writes nothing shared, so what a chunk makes depends on its pairs only; the cut
//! depends on the number of pairs and the chunk size, never on the workers, and the serial store
//! in chunk order is the order the serial loop pushes in, with the same count of manifolds that did
//! not fit.

use super::dim::Dim;
use super::execution::SolverExecution;
use super::manifolds::{ContactManifold, ManifoldList};
use super::pairs::Pair;
use base::fpenv::FpGuard;
use jobs::scheduler::IScheduler;
use std::sync::{Arc, Mutex, PoisonError};

/// The fewest chunks of pairs worth a hand-over to the scheduler.
const MIN_CHUNKS: usize = 2;

/// Chunks per scheduler job: each chunk is one index of the loop.
const INDICES_PER_JOB: u32 = 1;

/// The contact test of one pair: its manifold, or `None` when the pair makes none.
pub(super) type ContactOf<'a, D> = dyn Fn(&Pair<D>) -> Option<ContactManifold<D>> + Sync + 'a;

/// The scheduler and the buffers (one per chunk of the largest possible pair list) of the
/// contact tests; built when the execution has a scheduler, so a step allocates nothing.
pub(super) struct ContactRun<D: Dim> {
    scheduler: Arc<dyn IScheduler>,
    chunk: usize,
    buffers: Vec<Mutex<Vec<ContactManifold<D>>>>,
}

impl<D: Dim> ContactRun<D> {
    /// The run for `execution` and a pair list of at most `pair_capacity` pairs; `None` without a
    /// scheduler (the contact tests then stay on the calling thread).
    pub(super) fn new(execution: &SolverExecution, pair_capacity: usize) -> Option<Self> {
        let scheduler = Arc::clone(execution.scheduler.as_ref()?);
        let chunk = execution.chunk_size();
        let room = chunk.min(pair_capacity);
        let buffers = (0..pair_capacity.div_ceil(chunk))
            .map(|_| Mutex::new(Vec::with_capacity(room)))
            .collect();
        Some(Self {
            scheduler,
            chunk,
            buffers,
        })
    }

    /// Stores the manifolds of `pairs` into `manifolds`, in the order of `pairs`. Returns false,
    /// having stored nothing, when the pairs make a single chunk or more chunks than there are
    /// buffers (the caller then runs the serial loop).
    pub(super) fn run(
        &self,
        pairs: &[Pair<D>],
        contact: &ContactOf<'_, D>,
        manifolds: &mut ManifoldList<D>,
    ) -> bool {
        let chunks = pairs.len().div_ceil(self.chunk);
        let Ok(count) = u32::try_from(chunks) else {
            return false;
        };
        if chunks < MIN_CHUNKS || chunks > self.buffers.len() {
            return false;
        }
        let ran = self
            .scheduler
            .parallel_for(count, INDICES_PER_JOB, &|range| {
                range.for_each(|index| self.test_chunk(index as usize, pairs, contact));
            });
        // A refusing scheduler ran nothing, so the chunks run here (a chunk clears its own
        // buffer first, so running one twice is harmless).
        if ran.is_err() {
            (0..chunks).for_each(|index| self.test_chunk(index, pairs, contact));
        }
        for buffer in self.buffers.iter().take(chunks) {
            let buffer = buffer.lock().unwrap_or_else(PoisonError::into_inner);
            buffer.iter().for_each(|&entry| manifolds.push(entry));
        }
        true
    }

    /// Tests the pairs of chunk `index` into its buffer.
    fn test_chunk(&self, index: usize, pairs: &[Pair<D>], contact: &ContactOf<'_, D>) {
        let _guard = FpGuard::deterministic();
        let from = index.saturating_mul(self.chunk);
        let to = from.saturating_add(self.chunk).min(pairs.len());
        let (Some(part), Some(buffer)) = (pairs.get(from..to), self.buffers.get(index)) else {
            return;
        };
        let mut buffer = buffer.lock().unwrap_or_else(PoisonError::into_inner);
        buffer.clear();
        buffer.extend(part.iter().filter_map(contact));
    }
}
