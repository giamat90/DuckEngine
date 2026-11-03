//! The `IScheduler` port and the chunking rule every scheduler of this crate shares.

use crate::error::JobError;
use std::ops::Range;

/// The body of a parallel loop: called with disjoint, non-empty index ranges that together
/// cover `0..count`. It borrows the caller's data and must be `Sync` so that a later pool can
/// call it from several threads.
pub type RangeBody<'a> = dyn Fn(Range<u32>) + Sync + 'a;

/// One independent task of a scope (same borrowing and `Sync` rule as [`RangeBody`]).
pub type Task<'a> = dyn Fn() + Sync + 'a;

/// Calls the tasks named by `order`, in that order (indices past the end are skipped).
pub(crate) fn run_tasks(tasks: &[&Task<'_>], order: impl Iterator<Item = usize>) {
    for task in order.filter_map(|index| tasks.get(index)) {
        task();
    }
}

/// A loop of `count` indices cut into chunks of `grain` indices (the last may be shorter).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Chunks {
    count: u32,
    grain: u32,
}

impl Chunks {
    /// # Errors
    /// [`JobError::InvalidGrain`] when `grain` is zero.
    pub fn new(count: u32, grain: u32) -> Result<Self, JobError> {
        if grain == 0 {
            return Err(JobError::InvalidGrain);
        }
        Ok(Self { count, grain })
    }

    /// Number of chunks (zero when `count` is zero).
    #[must_use]
    pub fn len(self) -> u32 {
        self.count.div_ceil(self.grain)
    }

    /// Calls `body` on the chunks named by `order`, in that order (indices past the last chunk
    /// are skipped). The one loop every scheduler of this crate shares.
    pub fn run(self, order: impl Iterator<Item = u32>, body: &RangeBody<'_>) {
        for range in order.filter_map(|index| self.range(index)) {
            body(range);
        }
    }

    /// The index range of chunk `index`, or `None` past the last chunk.
    #[must_use]
    pub fn range(self, index: u32) -> Option<Range<u32>> {
        if index >= self.len() {
            return None;
        }
        // index < len, so index * grain < count and cannot overflow.
        let start = index * self.grain;
        let end = start.saturating_add(self.grain).min(self.count);
        Some(start..end)
    }
}

/// The job system port (ADR-0004): fork-join over borrowed closures.
///
/// # Contract
/// * **Join.** Every call returns only after all of its jobs have finished; the caller's writes
///   made by those jobs (to disjoint slots) are visible afterwards. For the inline scheduler the
///   join is trivially immediate, but callers must not rely on that.
/// * **Exactly once.** Each index of `0..count` is covered by exactly one chunk, and each task of
///   a scope runs exactly once. Chunks are non-empty and disjoint; every chunk except at most
///   one (the one that ends at `count`) has at least `grain` indices.
/// * **No order, no fixed grouping.** The order in which chunks or tasks run, the thread that
///   runs them and where chunk boundaries fall (any grouping with chunks of at least `grain`) are
///   not specified. A body's effect must equal calling it index by index, in any order and any
///   grouping, and it may write only slots that belong to its own indices. A body that reduces
///   per chunk (for example a float partial sum per chunk) depends on the grouping and breaks D1
///   across worker counts. The reversed and shuffled schedulers catch order dependence; the
///   contract suite proves that per-index loops are grouping independent.
/// * **No nesting.** A job must not call the scheduler it runs on. Any call may be refused with
///   [`JobError::Busy`] while another call is in progress (nested or from another thread); the
///   inline schedulers happen to accept nesting, which code must not rely on.
/// * **Errors are values.** A refused call (`Err`) has run no job. Jobs themselves cannot fail;
///   a panic in a job aborts the process (`panic = abort`), so none crosses this boundary.
/// * **No allocation.** A call allocates nothing on the heap after construction and warm-up.
/// * **Bounds.** `Send + Sync` so one scheduler is shared by `&dyn IScheduler` across threads,
///   and the closures are `Sync` so a pool can call one body from several workers while the
///   caller waits. No `'static` bound: closures borrow the caller's frame data.
pub trait IScheduler: Send + Sync {
    /// How many threads run jobs at the same time, the calling thread included (at least 1).
    fn worker_count(&self) -> u32;

    /// Runs `body` over `0..count` in chunks of `grain` indices (see the contract) and waits.
    ///
    /// # Errors
    /// [`JobError::InvalidGrain`] when `grain` is zero, or a scheduler-specific refusal; no job
    /// ran in that case.
    fn parallel_for(&self, count: u32, grain: u32, body: &RangeBody<'_>) -> Result<(), JobError>;

    /// Runs every task of `tasks` once, independent of each other, and waits for all of them.
    ///
    /// # Errors
    /// A scheduler-specific refusal (see [`JobError`]); no task ran in that case.
    fn run_scope(&self, tasks: &[&Task<'_>]) -> Result<(), JobError>;

    /// [`parallel_for`](Self::parallel_for) with `body` called once per index.
    ///
    /// # Errors
    /// As [`parallel_for`](Self::parallel_for).
    fn for_each(
        &self,
        count: u32,
        grain: u32,
        body: &(dyn Fn(u32) + Sync),
    ) -> Result<(), JobError> {
        self.parallel_for(count, grain, &|range| range.for_each(body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunks(count: u32, grain: u32) -> Chunks {
        Chunks::new(count, grain).unwrap_or_else(|error| panic!("valid chunking: {error}"))
    }

    #[test]
    fn a_zero_grain_is_refused() {
        assert_eq!(Chunks::new(10, 0), Err(JobError::InvalidGrain));
    }

    #[test]
    fn zero_indices_make_no_chunk() {
        let cut = chunks(0, 4);
        assert_eq!(cut.len(), 0);
        assert_eq!(cut.range(0), None);
    }

    #[test]
    fn the_last_chunk_is_the_short_one() {
        let cut = chunks(10, 4);
        assert_eq!(cut.len(), 3);
        assert_ne!(cut.range(0), None);
        assert_eq!(cut.len(), 3);
        assert_eq!(cut.range(0), Some(0..4));
        assert_eq!(cut.range(1), Some(4..8));
        assert_eq!(cut.range(2), Some(8..10));
        assert_eq!(cut.range(3), None);
    }

    #[test]
    fn a_grain_larger_than_the_count_makes_one_chunk() {
        let cut = chunks(3, u32::MAX);
        assert_eq!(cut.len(), 1);
        assert_eq!(cut.range(0), Some(0..3));
    }

    #[test]
    fn the_largest_loop_does_not_overflow() {
        let cut = chunks(u32::MAX, 1);
        assert_eq!(cut.len(), u32::MAX);
        assert_eq!(cut.range(u32::MAX - 1), Some(u32::MAX - 1..u32::MAX));
        let wide = chunks(u32::MAX, u32::MAX - 1);
        assert_eq!(wide.len(), 2);
        assert_eq!(wide.range(1), Some(u32::MAX - 1..u32::MAX));
    }
}
