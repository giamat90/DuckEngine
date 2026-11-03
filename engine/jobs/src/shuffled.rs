//! The shuffled test scheduler: runs every job on the calling thread in a seeded random order.

use crate::error::JobError;
use crate::scheduler::{Chunks, IScheduler, RangeBody, Task, run_tasks};
use base::rng::Pcg32;
use std::sync::{Mutex, MutexGuard, TryLockError};

/// The generator and the permutation buffer, filled before a call and read while it runs.
struct State {
    rng: Pcg32,
    order: Vec<u32>,
}

/// Runs chunks and tasks in an order drawn from a seeded `Pcg32`, a fresh permutation per call,
/// so the same seed and the same sequence of calls always give the same orders. A body that gives
/// another result than under the inline scheduler depends on the order. Test tool.
///
/// The permutation buffer is allocated once in [`new`](Self::new); a call with more chunks than
/// that is refused (`TooManyChunks`) instead of allocating. A job cannot start a nested call on
/// the same scheduler: it gets `Busy` (the buffer is in use).
pub struct ShuffledScheduler {
    max_chunks: u32,
    state: Mutex<State>,
}

impl ShuffledScheduler {
    /// A scheduler that orders up to `max_chunks` chunks (or tasks) per call.
    #[must_use]
    pub fn new(seed: u64, max_chunks: u32) -> Self {
        Self {
            max_chunks,
            state: Mutex::new(State {
                rng: Pcg32::new(seed, 0),
                order: Vec::with_capacity(max_chunks as usize),
            }),
        }
    }

    /// Takes the state and fills `order` with a fresh permutation of `0..len`.
    fn shuffled(&self, len: u32) -> Result<MutexGuard<'_, State>, JobError> {
        if len > self.max_chunks {
            return Err(JobError::TooManyChunks {
                max: self.max_chunks,
            });
        }
        let mut state = match self.state.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return Err(JobError::Busy),
        };
        state.order.clear();
        state.order.extend(0..len);
        // Fisher-Yates: position `top` swaps with a uniformly drawn position at or below it.
        for top in (1..len).rev() {
            // `top + 1` is at least 2, so `below` cannot report a zero bound.
            let other = state.rng.below(top + 1).unwrap_or(top);
            state.order.swap(top as usize, other as usize);
        }
        Ok(state)
    }
}

impl IScheduler for ShuffledScheduler {
    fn worker_count(&self) -> u32 {
        1
    }

    fn parallel_for(&self, count: u32, grain: u32, body: &RangeBody<'_>) -> Result<(), JobError> {
        let chunks = Chunks::new(count, grain)?;
        let state = self.shuffled(chunks.len())?;
        chunks.run(state.order.iter().copied(), body);
        Ok(())
    }

    fn run_scope(&self, tasks: &[&Task<'_>]) -> Result<(), JobError> {
        // A slice longer than u32::MAX is over every limit, so report it as such.
        let len = u32::try_from(tasks.len()).map_err(|_| JobError::TooManyChunks {
            max: self.max_chunks,
        })?;
        let state = self.shuffled(len)?;
        run_tasks(tasks, state.order.iter().map(|&index| index as usize));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{
        observed_chunk_order, observed_task_order, order_independence_contract, scheduler_contract,
    };
    use crate::inline::InlineScheduler;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn the_shuffled_scheduler_passes_the_contract() {
        scheduler_contract(&ShuffledScheduler::new(7, 64));
    }

    #[test]
    fn chunks_run_in_a_permutation_that_is_neither_ascending_nor_descending() {
        let scheduler = ShuffledScheduler::new(1, 64);
        let starts = observed_chunk_order(&scheduler, 64, 4);
        let mut sorted = starts.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..16).map(|chunk| chunk * 4).collect::<Vec<_>>());
        let reversed: Vec<u32> = sorted.iter().rev().copied().collect();
        assert_ne!(starts, sorted);
        assert_ne!(starts, reversed);
    }

    #[test]
    fn the_same_seed_gives_the_same_orders_and_another_seed_another() {
        let orders = |seed| {
            let scheduler = ShuffledScheduler::new(seed, 64);
            (
                observed_chunk_order(&scheduler, 64, 4),
                observed_chunk_order(&scheduler, 64, 4),
            )
        };
        assert_eq!(orders(5), orders(5));
        assert_ne!(orders(5), orders(6));
        let (first, second) = orders(5);
        assert_ne!(first, second, "each call draws a fresh permutation");
    }

    #[test]
    fn tasks_run_in_a_seeded_permutation() {
        let order = |seed| observed_task_order(&ShuffledScheduler::new(seed, 16), 12);
        let mut sorted = order(3);
        assert_eq!(order(3), sorted);
        assert_ne!(order(3), order(4));
        sorted.sort_unstable();
        assert_eq!(sorted, (0..12).collect::<Vec<_>>());
        assert_ne!(order(3), (0..12).collect::<Vec<_>>());
    }

    #[test]
    fn every_order_of_three_chunks_occurs() {
        let mut seen: Vec<Vec<u32>> = (0..200)
            .map(|seed| observed_chunk_order(&ShuffledScheduler::new(seed, 8), 3, 1))
            .collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 6, "all six permutations must be reachable");
    }

    #[test]
    fn a_call_with_more_chunks_than_the_limit_is_refused_and_runs_nothing() {
        let scheduler = ShuffledScheduler::new(1, 4);
        let calls = AtomicU32::new(0);
        let count_call = || {
            calls.fetch_add(1, Ordering::Relaxed);
        };
        let refused = scheduler.parallel_for(5, 1, &|_| count_call());
        assert_eq!(refused, Err(JobError::TooManyChunks { max: 4 }));
        let tasks: [&Task<'_>; 5] = [&count_call; 5];
        assert_eq!(
            scheduler.run_scope(&tasks),
            Err(JobError::TooManyChunks { max: 4 })
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert_eq!(scheduler.parallel_for(4, 1, &|_| count_call()), Ok(()));
        assert_eq!(calls.load(Ordering::Relaxed), 4);
    }

    #[test]
    fn a_nested_call_is_refused_as_busy_and_the_outer_call_finishes() {
        let scheduler = ShuffledScheduler::new(1, 8);
        let busy = AtomicU32::new(0);
        let outer = scheduler.parallel_for(3, 1, &|_| {
            let nested = scheduler.parallel_for(2, 1, &|_| ());
            busy.fetch_add(u32::from(nested == Err(JobError::Busy)), Ordering::Relaxed);
        });
        assert_eq!(outer, Ok(()));
        assert_eq!(busy.load(Ordering::Relaxed), 3);
        assert_eq!(scheduler.parallel_for(2, 1, &|_| ()), Ok(()));
    }

    #[test]
    fn an_empty_call_succeeds_and_runs_nothing() {
        let scheduler = ShuffledScheduler::new(1, 0);
        assert_eq!(scheduler.parallel_for(0, 3, &|_| unreachable!()), Ok(()));
        assert_eq!(scheduler.run_scope(&[]), Ok(()));
    }

    #[test]
    fn a_pure_loop_gives_the_inline_result_under_many_seeds() {
        for seed in 0..20 {
            order_independence_contract(&[&InlineScheduler, &ShuffledScheduler::new(seed, 64)]);
        }
    }
}
