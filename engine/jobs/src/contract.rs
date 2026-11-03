//! Generic scheduler contract suite: each function takes the scheduler under test, so every
//! scheduler (inline, reversed, shuffled, and the pool of card 4-7) runs the same checks
//! (design: `wiki/architecture/physics-and-jobs-v0.md` sections 4 and 7). A new scheduler is
//! accepted when its suite passes unchanged.
//!
//! The suite proves what the port promises and nothing more: it never asserts an execution order.

use crate::error::JobError;
use crate::scheduler::{IScheduler, Task};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

/// Loop shapes the suite runs: `(count, grain)`. They cover an empty loop, a single index, a grain
/// of one, a grain that divides the count, one that leaves a short last chunk, and a grain larger
/// than the count.
const SHAPES: [(u32, u32); 7] = [
    (0, 4),
    (1, 1),
    (7, 1),
    (16, 4),
    (17, 4),
    (5, 64),
    (1000, 33),
];

fn counters(len: u32) -> Vec<AtomicU32> {
    (0..len).map(|_| AtomicU32::new(0)).collect()
}

/// The whole suite: worker count, refusals, `parallel_for` and `run_scope`, repeated use.
///
/// # Panics
/// When the scheduler breaks the contract (this is test code).
pub fn scheduler_contract(scheduler: &dyn IScheduler) {
    assert!(
        scheduler.worker_count() >= 1,
        "worker_count must be at least 1"
    );
    refusal_contract(scheduler);
    for (count, grain) in SHAPES {
        parallel_for_contract(scheduler, count, grain);
    }
    run_scope_contract(scheduler);
    // A scheduler must stay usable: the pool reuses its queues from call to call.
    for _ in 0..50 {
        parallel_for_contract(scheduler, 17, 4);
        run_scope_contract(scheduler);
    }
}

/// A zero grain is `Err(InvalidGrain)` and runs nothing.
///
/// # Panics
/// When the scheduler accepts a zero grain or runs a job anyway.
pub fn refusal_contract(scheduler: &dyn IScheduler) {
    let calls = AtomicU32::new(0);
    let result = scheduler.parallel_for(10, 0, &|_| {
        calls.fetch_add(1, Ordering::Relaxed);
    });
    assert_eq!(
        result,
        Err(JobError::InvalidGrain),
        "a zero grain must be refused"
    );
    assert_eq!(
        calls.load(Ordering::Relaxed),
        0,
        "a refused call must run no job"
    );
}

/// `parallel_for(count, grain)`: chunks are non-empty and inside `0..count`, only the chunk that
/// ends at `count` may be shorter than `grain`, every index runs exactly once, and when the call
/// returns the writes of the jobs to their own slots are visible.
///
/// # Panics
/// When the scheduler breaks the contract.
pub fn parallel_for_contract(scheduler: &dyn IScheduler, count: u32, grain: u32) {
    let hits = counters(count);
    let slots = counters(count);
    let bad_chunks = AtomicU32::new(0);
    let result = scheduler.parallel_for(count, grain, &|range| {
        let malformed = range.start >= range.end || range.end > count;
        let short = range.end.saturating_sub(range.start) < grain && range.end != count;
        if malformed || short {
            bad_chunks.fetch_add(1, Ordering::Relaxed);
            return;
        }
        for index in range {
            hits[index as usize].fetch_add(1, Ordering::Relaxed);
            slots[index as usize].store(index * 3 + 1, Ordering::Relaxed);
        }
    });
    assert_eq!(result, Ok(()), "a valid call must succeed");
    // The reads below happen after the call returned: the join must have made the writes visible.
    assert_eq!(
        bad_chunks.load(Ordering::Relaxed),
        0,
        "chunks must be non-empty, inside the range, and long enough"
    );
    for index in 0..count {
        let i = index as usize;
        assert_eq!(
            hits[i].load(Ordering::Relaxed),
            1,
            "index {index} must run exactly once (count {count}, grain {grain})"
        );
        assert_eq!(
            slots[i].load(Ordering::Relaxed),
            index * 3 + 1,
            "the write to slot {index} must be visible after the join"
        );
    }
}

/// `run_scope`: an empty scope succeeds, and every task runs exactly once before it returns.
///
/// # Panics
/// When the scheduler breaks the contract.
pub fn run_scope_contract(scheduler: &dyn IScheduler) {
    assert_eq!(
        scheduler.run_scope(&[]),
        Ok(()),
        "an empty scope must succeed"
    );
    let runs = counters(3);
    let first = || {
        runs[0].fetch_add(1, Ordering::Relaxed);
    };
    let second = || {
        runs[1].fetch_add(1, Ordering::Relaxed);
    };
    let third = || {
        runs[2].fetch_add(1, Ordering::Relaxed);
    };
    let tasks: [&Task<'_>; 3] = [&first, &second, &third];
    assert_eq!(
        scheduler.run_scope(&tasks),
        Ok(()),
        "a valid scope must succeed"
    );
    for (index, run) in runs.iter().enumerate() {
        assert_eq!(
            run.load(Ordering::Relaxed),
            1,
            "task {index} must run exactly once"
        );
    }
}

/// The values a pure per-index function gives, computed through `scheduler`.
///
/// # Panics
/// When the scheduler refuses a valid call.
#[must_use]
pub fn pure_loop_result(scheduler: &dyn IScheduler, count: u32, grain: u32) -> Vec<u32> {
    let slots = counters(count);
    let result = scheduler.for_each(count, grain, &|index| {
        slots[index as usize].store(mix(index), Ordering::Relaxed);
    });
    assert_eq!(result, Ok(()), "a valid call must succeed");
    slots
        .iter()
        .map(|slot| slot.load(Ordering::Relaxed))
        .collect()
}

/// A cheap integer hash, pure in `index`.
fn mix(index: u32) -> u32 {
    let spread = index.wrapping_mul(0x9e37_79b1);
    spread ^ (spread >> 15)
}

/// Order independence: a loop whose body is a pure function of the index gives the same result
/// under every scheduler as under the first one (give it the inline scheduler first).
///
/// # Panics
/// When two schedulers give different results.
pub fn order_independence_contract(schedulers: &[&dyn IScheduler]) {
    let Some((reference, others)) = schedulers.split_first() else {
        return;
    };
    for (count, grain) in SHAPES {
        let expected = pure_loop_result(*reference, count, grain);
        for (position, other) in others.iter().enumerate() {
            assert_eq!(
                pure_loop_result(*other, count, grain),
                expected,
                "scheduler {} gave another result (count {count}, grain {grain})",
                position + 1
            );
        }
    }
}

/// The start index of each chunk in the order `scheduler` ran them (for tests of the order
/// schedulers; the port itself promises no order).
///
/// # Panics
/// When the scheduler refuses a valid call.
#[must_use]
pub fn observed_chunk_order(scheduler: &dyn IScheduler, count: u32, grain: u32) -> Vec<u32> {
    let order = Mutex::new(Vec::new());
    let result = scheduler.parallel_for(count, grain, &|range| {
        record(&order, range.start);
    });
    assert_eq!(result, Ok(()), "a valid call must succeed");
    order.into_inner().unwrap_or_default()
}

/// The index of each task in the order `scheduler` ran a scope of `len` tasks.
///
/// # Panics
/// When the scheduler refuses a valid scope.
#[must_use]
pub fn observed_task_order(scheduler: &dyn IScheduler, len: usize) -> Vec<usize> {
    let order = Mutex::new(Vec::new());
    run_recording_scope(scheduler, len, &order);
    order.into_inner().unwrap_or_default()
}

fn record<T>(order: &Mutex<Vec<T>>, value: T) {
    if let Ok(mut seen) = order.lock() {
        seen.push(value);
    }
}

fn run_recording_scope(scheduler: &dyn IScheduler, len: usize, order: &Mutex<Vec<usize>>) {
    let closures: Vec<Box<Task<'_>>> = (0..len)
        .map(|index| -> Box<Task<'_>> { Box::new(move || record(order, index)) })
        .collect();
    let tasks: Vec<&Task<'_>> = closures.iter().map(|task| &**task).collect();
    assert_eq!(
        scheduler.run_scope(&tasks),
        Ok(()),
        "a valid scope must succeed"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::faulty::{Fault, Faulty, Merging};
    use crate::inline::InlineScheduler;

    #[test]
    fn the_inline_scheduler_passes_the_contract() {
        scheduler_contract(&InlineScheduler);
    }

    #[test]
    fn the_inline_scheduler_runs_chunks_and_tasks_in_ascending_order() {
        assert_eq!(
            observed_chunk_order(&InlineScheduler, 10, 3),
            vec![0, 3, 6, 9]
        );
        assert_eq!(observed_task_order(&InlineScheduler, 4), vec![0, 1, 2, 3]);
    }

    #[test]
    fn the_pure_loop_result_is_the_hash_of_each_index() {
        assert_eq!(
            pure_loop_result(&InlineScheduler, 4, 3),
            vec![0, 0x9e36_45df, 0x3c6e_8bbf, 0xdaa7_d85f]
        );
    }

    #[test]
    fn a_scheduler_that_merges_chunks_passes_and_gives_the_inline_result() {
        scheduler_contract(&Merging);
        order_independence_contract(&[&InlineScheduler, &Merging]);
    }

    #[test]
    fn the_control_scheduler_without_a_fault_passes_the_suite() {
        scheduler_contract(&Faulty(Fault::None));
    }

    #[test]
    #[should_panic(expected = "must run exactly once")]
    fn a_scheduler_that_skips_a_chunk_fails_the_suite() {
        scheduler_contract(&Faulty(Fault::SkipsLastChunk));
    }

    #[test]
    #[should_panic(expected = "must run exactly once")]
    fn a_scheduler_that_runs_a_chunk_twice_fails_the_suite() {
        scheduler_contract(&Faulty(Fault::RunsFirstChunkTwice));
    }

    #[test]
    #[should_panic(expected = "a zero grain must be refused")]
    fn a_scheduler_that_accepts_a_zero_grain_fails_the_suite() {
        scheduler_contract(&Faulty(Fault::AcceptsZeroGrain));
    }

    #[test]
    #[should_panic(expected = "non-empty, inside the range, and long enough")]
    fn a_scheduler_that_cuts_chunks_below_the_grain_fails_the_suite() {
        scheduler_contract(&Faulty(Fault::SplitsTooFine));
    }

    #[test]
    #[should_panic(expected = "non-empty, inside the range, and long enough")]
    fn a_scheduler_that_overruns_the_range_fails_the_suite() {
        scheduler_contract(&Faulty(Fault::OverrunsTheRange));
    }

    #[test]
    #[should_panic(expected = "task 2 must run exactly once")]
    fn a_scheduler_that_skips_a_task_fails_the_suite() {
        scheduler_contract(&Faulty(Fault::SkipsLastTask));
    }

    #[test]
    #[should_panic(expected = "task 0 must run exactly once")]
    fn a_scheduler_that_runs_a_task_twice_fails_the_suite() {
        scheduler_contract(&Faulty(Fault::RunsFirstTaskTwice));
    }

    #[test]
    #[should_panic(expected = "a refused call must run no job")]
    fn a_scheduler_that_runs_a_job_while_refusing_fails_the_suite() {
        scheduler_contract(&Faulty(Fault::RefusesButRuns));
    }

    #[test]
    #[should_panic(expected = "gave another result")]
    fn order_independence_notices_a_different_result() {
        order_independence_contract(&[&InlineScheduler, &Faulty(Fault::SkipsLastChunk)]);
    }

    #[test]
    fn order_independence_of_nothing_passes() {
        order_independence_contract(&[]);
    }
}
