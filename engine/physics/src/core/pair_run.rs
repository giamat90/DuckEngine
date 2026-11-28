//! The pair search of the broadphase through a scheduler (card 8-111): the slots of the world are
//! cut into fixed chunks, each chunk asks the trees for the pairs of its dynamic bodies into a buffer
//! of its own, and the buffers are stored into the pair list serially, in chunk order, which is the
//! order the serial search finds them in (dynamic bodies in slot order).
//!
//! Why it equals the serial search bit for bit: a query reads the two trees and the slots and
//! writes nothing shared, so what a chunk finds depends on its slots only; the cut depends on the
//! number of slots and the chunk size, never on the workers. The list keeps the first pairs of the
//! search order when it is too small, and the store is in that order, so even then the list and its
//! count of lost pairs are the serial ones. When a chunk finds more than its buffer holds, the
//! parallel result is thrown away and the serial search runs instead. That case is rare (a buffer
//! holds 16 pairs per body of its chunk) and costs only speed.

use super::body::{BodyHandle, BodyKind};
use super::dim::Dim;
use super::execution::SolverExecution;
use super::pairs::PairList;
use super::tree::Tree;
use super::world::Slot;
use base::fpenv::FpGuard;
use jobs::scheduler::IScheduler;
use std::sync::{Arc, Mutex, PoisonError};

/// The fewest chunks of slots worth a hand-over to the scheduler.
const MIN_CHUNKS: usize = 2;

/// Chunks per scheduler job: each chunk is one index of the loop.
const INDICES_PER_JOB: u32 = 1;

/// How many pairs per body of its chunk a chunk may find before the search goes serial: a body
/// with more than this on average (a dense pile) makes the buffers overflow and the search falls
/// back to the serial one. The buffers hold this many pairs per slot of the world.
const PAIRS_PER_BODY: usize = 16;

/// The two trees and the slots a search reads.
pub(super) struct Search<'a, D: Dim> {
    pub dynamic_tree: &'a Tree<D::Bounds>,
    pub static_tree: &'a Tree<D::Bounds>,
    pub slots: &'a [Option<Slot<D>>],
}

impl<D: Dim> Search<'_, D> {
    /// Calls `found(own, other)` for every pair the dynamic body in `slot` has with a body whose
    /// box overlaps its own: with the dynamic bodies of a higher slot (so that two dynamic bodies
    /// report their pair once, from the lower one), then with every static body that overlaps.
    fn of(&self, slot: &Slot<D>, mut found: impl FnMut(BodyHandle<D>, BodyHandle<D>)) {
        let Ok(region) = self.dynamic_tree.fat_bounds(slot.proxy) else {
            return;
        };
        let own = slot.handle;
        self.dynamic_tree
            .query(&region, |other| self.later_hit(own, other, &mut found));
        self.static_tree
            .query(&region, |other| self.hit(own, other, &mut found));
    }

    /// Reports the pair of `own` and the body in slot `other`, if there is one.
    fn hit(
        &self,
        own: BodyHandle<D>,
        other: u32,
        found: &mut impl FnMut(BodyHandle<D>, BodyHandle<D>),
    ) {
        if let Some(Some(slot)) = self.slots.get(other as usize) {
            found(own, slot.handle);
        }
    }

    /// Like [`Self::hit`], but only for a body of a higher slot than `own`.
    fn later_hit(
        &self,
        own: BodyHandle<D>,
        other: u32,
        found: &mut impl FnMut(BodyHandle<D>, BodyHandle<D>),
    ) {
        if other > own.index() {
            self.hit(own, other, found);
        }
    }

    /// Stores the pairs of every dynamic body of `part` (slots, in slot order) into `list`.
    fn all_into(&self, part: &[Option<Slot<D>>], list: &mut PairList<D>) {
        for slot in part.iter().flatten().filter(|slot| is_dynamic(slot)) {
            self.of(slot, |a, b| list.push(a, b));
        }
    }

    /// Stores the pairs of every dynamic body of `part` into `found`.
    fn chunk_into(&self, part: &[Option<Slot<D>>], limit: usize, found: &mut Found<D>) {
        for slot in part.iter().flatten().filter(|slot| is_dynamic(slot)) {
            self.of_unless_full(slot, limit, found);
        }
    }

    /// Searches for `slot` unless the buffer already overflowed: the whole parallel result will
    /// then be thrown away, so the rest of the chunk is skipped.
    fn of_unless_full(&self, slot: &Slot<D>, limit: usize, found: &mut Found<D>) {
        if !found.overflowed {
            self.of(slot, |a, b| found.add(a, b, limit));
        }
    }

    /// The serial search: every dynamic body of the world, in slot order.
    pub(super) fn serial(&self, list: &mut PairList<D>) {
        self.all_into(self.slots, list);
    }
}

fn is_dynamic<D: Dim>(slot: &Slot<D>) -> bool {
    slot.body.kind() == BodyKind::Dynamic
}

/// What one chunk found: its pairs in search order, and whether it found more than it can hold.
struct Found<D: Dim> {
    pairs: Vec<(BodyHandle<D>, BodyHandle<D>)>,
    overflowed: bool,
}

impl<D: Dim> Found<D> {
    /// Keeps the pair, or notes that the buffer is full (`limit` pairs) and the pair is lost.
    fn add(&mut self, a: BodyHandle<D>, b: BodyHandle<D>, limit: usize) {
        if self.pairs.len() < limit {
            self.pairs.push((a, b));
        } else {
            self.overflowed = true;
        }
    }
}

/// The scheduler and the buffers (one per chunk of slots) of the pair search; built when the
/// execution has a scheduler, so a step allocates nothing.
pub(super) struct PairRun<D: Dim> {
    scheduler: Arc<dyn IScheduler>,
    chunk: usize,
    limit: usize,
    buffers: Vec<Mutex<Found<D>>>,
}

impl<D: Dim> PairRun<D> {
    /// The run for `execution`, a world of `max_bodies` slots and a pair list of `pair_capacity`;
    /// `None` without a scheduler or when the slots make a single chunk (the search then stays on
    /// the calling thread).
    pub(super) fn new(
        execution: &SolverExecution,
        max_bodies: usize,
        pair_capacity: usize,
    ) -> Option<Self> {
        let scheduler = Arc::clone(execution.scheduler.as_ref()?);
        let chunk = execution.chunk_size();
        let count = max_bodies.div_ceil(chunk);
        if count < MIN_CHUNKS {
            // One chunk of slots is never handed over, so it needs no buffer.
            return None;
        }
        // More pairs than the list holds in one chunk are not worth a buffer: the serial search runs.
        let limit = chunk
            .min(max_bodies)
            .saturating_mul(PAIRS_PER_BODY)
            .min(pair_capacity)
            .max(1);
        let buffers = (0..count)
            .map(|_| {
                Mutex::new(Found {
                    pairs: Vec::with_capacity(limit),
                    overflowed: false,
                })
            })
            .collect();
        Some(Self {
            scheduler,
            chunk,
            limit,
            buffers,
        })
    }

    /// Stores the pairs of the dynamic bodies of `slots` into `list` (which the caller emptied), in
    /// the order of the serial search. Returns false, having stored nothing, when the slots make a
    /// single chunk or when a chunk found more than its buffer holds (the caller then runs the
    /// serial search).
    pub(super) fn run(&self, search: &Search<'_, D>, list: &mut PairList<D>) -> bool {
        let chunks = search.slots.len().div_ceil(self.chunk);
        let Ok(count) = u32::try_from(chunks) else {
            return false;
        };
        if chunks < MIN_CHUNKS || chunks > self.buffers.len() {
            return false;
        }
        let run_chunk = |index: usize| self.search_chunk(index, search);
        let ran = self
            .scheduler
            .parallel_for(count, INDICES_PER_JOB, &|range| {
                range.for_each(|index| run_chunk(index as usize));
            });
        // A refusing scheduler ran nothing, so the chunks run here (a chunk clears its own
        // buffer first, so running one twice is harmless).
        if ran.is_err() {
            (0..chunks).for_each(run_chunk);
        }
        self.store(chunks, list)
    }

    /// Stores the buffers of the first `chunks` chunks into `list` in order, unless one overflowed.
    /// A list that is too small takes the first pairs of that order, which is the order the serial
    /// search finds them in, and counts the rest, exactly as the serial search does.
    fn store(&self, chunks: usize, list: &mut PairList<D>) -> bool {
        let found = || {
            self.buffers
                .iter()
                .take(chunks)
                .map(|buffer| buffer.lock().unwrap_or_else(PoisonError::into_inner))
        };
        if found().any(|buffer| buffer.overflowed) {
            return false;
        }
        for buffer in found() {
            buffer.pairs.iter().for_each(|&(a, b)| list.push(a, b));
        }
        true
    }

    /// Searches the dynamic bodies of chunk `index` into its buffer.
    fn search_chunk(&self, index: usize, search: &Search<'_, D>) {
        let _guard = FpGuard::deterministic();
        let from = index.saturating_mul(self.chunk);
        let to = from.saturating_add(self.chunk).min(search.slots.len());
        let (Some(part), Some(buffer)) = (search.slots.get(from..to), self.buffers.get(index))
        else {
            return;
        };
        let mut buffer = buffer.lock().unwrap_or_else(PoisonError::into_inner);
        buffer.pairs.clear();
        buffer.overflowed = false;
        search.chunk_into(part, self.limit, &mut buffer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::d2::D2;
    use base::pool::Handle;
    use jobs::inline::InlineScheduler;

    fn scheduled(chunk: u32) -> SolverExecution {
        SolverExecution {
            scheduler: Some(Arc::new(InlineScheduler)),
            chunk,
            ..SolverExecution::SERIAL
        }
    }

    fn run(chunk: u32, max_bodies: usize, pair_capacity: usize) -> Option<PairRun<D2>> {
        PairRun::new(&scheduled(chunk), max_bodies, pair_capacity)
    }

    #[test]
    fn there_is_no_run_without_a_scheduler() {
        let none = PairRun::<D2>::new(&SolverExecution::SERIAL, 100, 100);
        assert!(none.is_none());
    }

    #[test]
    fn there_is_one_buffer_per_chunk_of_slots_each_holding_sixteen_pairs_a_body() {
        let found = run(4, 10, 1000);
        assert_eq!(found.as_ref().map(|run| run.buffers.len()), Some(3));
        assert_eq!(
            found.as_ref().map(|run| run.limit),
            Some(4 * PAIRS_PER_BODY)
        );
    }

    #[test]
    fn a_buffer_never_holds_more_than_the_list_does() {
        let found = run(4, 10, 20);
        assert_eq!(found.as_ref().map(|run| run.limit), Some(20));
        let zero = run(4, 10, 0);
        assert_eq!(zero.as_ref().map(|run| run.limit), Some(1));
    }

    #[test]
    fn a_pair_past_the_limit_is_not_stored_and_marks_the_buffer() {
        let pair = Handle::from_raw(1, 0);
        let mut found = Found::<D2> {
            pairs: Vec::new(),
            overflowed: false,
        };
        found.add(pair, pair, 2);
        found.add(pair, pair, 2);
        assert!(!found.overflowed && found.pairs.len() == 2);
        found.add(pair, pair, 2);
        assert!(found.overflowed && found.pairs.len() == 2);
    }
}
