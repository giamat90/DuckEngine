//! Every candidate passes the `jobs` scheduler contract suite unchanged, at several worker counts,
//! gives the serial hash on every synthetic workload (D1), refuses nesting, and makes no
//! allocator call on the calling thread after construction.

use base::alloc::{CountingAllocator, count_allocations};
use jobs::contract::{order_independence_contract, scheduler_contract};
use jobs::error::JobError;
use jobs::inline::InlineScheduler;
use jobs::scheduler::{IScheduler, Task};
use platform::threads::{NullThreads, StdThreads};
use std::alloc::System;
use std::sync::atomic::{AtomicU64, Ordering};
use tools_scheduler_spike::workloads::{Bench, Sizes, Workload};
use tools_scheduler_spike::{Kind, build};

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

const WORKER_COUNTS: [u32; 5] = [1, 2, 3, 4, 7];
const CANDIDATES: [Kind; 4] = [
    Kind::SharedQueue,
    Kind::MutexDeques,
    Kind::LockFree,
    Kind::StaticPartition,
];

fn scheduler(kind: Kind, workers: u32, spin: u32) -> Box<dyn IScheduler> {
    build(kind, workers, spin, &StdThreads::new())
        .unwrap_or_else(|error| panic!("{} with {workers} workers: {error}", kind.name()))
}

fn passes_the_suite(kind: Kind) {
    for workers in WORKER_COUNTS {
        let pool = scheduler(kind, workers, 0);
        assert_eq!(pool.worker_count(), workers);
        scheduler_contract(&*pool);
    }
}

#[test]
fn the_shared_queue_passes_the_contract_suite_at_every_worker_count() {
    passes_the_suite(Kind::SharedQueue);
}

#[test]
fn the_mutex_deques_pass_the_contract_suite_at_every_worker_count() {
    passes_the_suite(Kind::MutexDeques);
}

#[test]
fn the_lock_free_deques_pass_the_contract_suite_at_every_worker_count() {
    passes_the_suite(Kind::LockFree);
}

#[test]
fn the_static_partition_passes_the_contract_suite_at_every_worker_count() {
    passes_the_suite(Kind::StaticPartition);
}

#[test]
fn spinning_workers_pass_the_contract_suite_too() {
    for kind in [Kind::MutexDeques, Kind::LockFree, Kind::StaticPartition] {
        scheduler_contract(&*scheduler(kind, 4, 2000));
    }
}

#[test]
fn a_zero_worker_count_means_one() {
    assert_eq!(scheduler(Kind::LockFree, 0, 0).worker_count(), 1);
}

#[test]
fn every_candidate_gives_the_same_loop_result_as_the_inline_scheduler() {
    let pools: Vec<_> = CANDIDATES
        .iter()
        .map(|kind| scheduler(*kind, 4, 0))
        .collect();
    let mut all: Vec<&dyn IScheduler> = vec![&InlineScheduler];
    all.extend(pools.iter().map(|pool| &**pool));
    order_independence_contract(&all);
}

fn hashes_repeat(workload: Workload, kind: Kind, workers: u32) {
    let bench = Bench::new(workload, Sizes::SMALL);
    let reference = bench.run(&InlineScheduler);
    assert!(reference.is_ok());
    let pool = scheduler(kind, workers, 0);
    let runs: Vec<_> = (0..3).map(|_| bench.run(&*pool)).collect();
    assert!(
        runs.iter().all(|run| *run == reference),
        "{} {} workers {workers}",
        workload.name(),
        kind.name()
    );
}

fn workload_matches_everywhere(workload: Workload) {
    for kind in CANDIDATES {
        for workers in WORKER_COUNTS {
            hashes_repeat(workload, kind, workers);
        }
    }
}

#[test]
fn the_tiny_workload_gives_the_serial_hash_everywhere() {
    workload_matches_everywhere(Workload::Tiny);
    workload_matches_everywhere(Workload::TinyCoarse);
}

#[test]
fn the_balanced_and_imbalanced_workloads_give_the_serial_hash_everywhere() {
    workload_matches_everywhere(Workload::Balanced);
    workload_matches_everywhere(Workload::Imbalanced);
}

#[test]
fn the_dispatch_and_phases_workloads_give_the_serial_hash_everywhere() {
    workload_matches_everywhere(Workload::Dispatch);
    workload_matches_everywhere(Workload::Phases);
}

fn nested_call_is_refused(kind: Kind) {
    let pool = scheduler(kind, 3, 0);
    let refused = AtomicU64::new(0);
    let result = pool.parallel_for(8, 1, &|_| {
        let inner = pool.parallel_for(4, 1, &|_| {});
        refused.fetch_add(u64::from(inner == Err(JobError::Busy)), Ordering::Relaxed);
    });
    assert_eq!(result, Ok(()));
    assert_eq!(refused.load(Ordering::Relaxed), 8, "{}", kind.name());
}

#[test]
fn a_nested_call_is_refused_not_deadlocked() {
    CANDIDATES.into_iter().for_each(nested_call_is_refused);
}

#[test]
fn without_threads_a_pool_cannot_be_built_but_one_worker_needs_none() {
    for kind in CANDIDATES {
        assert!(build(kind, 2, 0, &NullThreads).is_err(), "{}", kind.name());
        assert!(build(kind, 1, 0, &NullThreads).is_ok(), "{}", kind.name());
    }
}

fn frame(pool: &dyn IScheduler, sum: &AtomicU64) {
    let each = pool.for_each(100, 7, &|index| {
        sum.fetch_add(u64::from(index), Ordering::Relaxed);
    });
    let task = || {
        sum.fetch_add(1, Ordering::Relaxed);
    };
    let tasks: [&Task<'_>; 2] = [&task, &task];
    assert_eq!((each, pool.run_scope(&tasks)), (Ok(()), Ok(())));
}

fn allocations_in_steady_state(kind: Kind) -> u64 {
    let pool = scheduler(kind, 4, 0);
    let sum = AtomicU64::new(0);
    frame(&*pool, &sum);
    let ((), counts) = count_allocations(|| (0..100).for_each(|_| frame(&*pool, &sum)));
    counts.calls()
}

#[test]
fn the_calling_thread_allocates_nothing_after_construction() {
    let ((), seen) = count_allocations(|| drop(std::hint::black_box(vec![1_u8; 16])));
    assert!(seen.calls() >= 1, "the counting allocator is not installed");
    for kind in CANDIDATES {
        assert_eq!(allocations_in_steady_state(kind), 0, "{}", kind.name());
    }
}
