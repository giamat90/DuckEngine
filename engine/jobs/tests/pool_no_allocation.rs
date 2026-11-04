//! The work-stealing pool allocates nothing per fork-join call after construction: not on the
//! calling thread (counted over the steady state after a warm-up) and not on any worker thread
//! (counted over the whole life of the thread, warm-up, parking and shutdown included).
//! Counting allocator with per-thread counters, as the other allocation tests do.

use base::alloc::{AllocCounts, CountingAllocator, count_allocations, current_counts};
use jobs::pool::{WaitPolicy, WorkStealingPool};
use jobs::scheduler::{IScheduler, Task};
use platform::error::PlatformError;
use platform::threads::{IThreads, Job, StdThreads, ThreadHandle};
use std::alloc::System;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

/// Frames of the steady state.
const FRAMES: usize = 200;

/// Threads of the operating system whose worker jobs report the allocator calls the thread made
/// while the job ran.
struct MeasuredThreads {
    inner: StdThreads,
    counts: Arc<Mutex<Vec<AllocCounts>>>,
}

impl MeasuredThreads {
    fn new() -> Self {
        Self {
            inner: StdThreads::new(),
            counts: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn worker_counts(&self) -> Vec<AllocCounts> {
        self.counts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl IThreads for MeasuredThreads {
    fn spawn(&self, name: &str, job: Job) -> Result<ThreadHandle, PlatformError> {
        let counts = Arc::clone(&self.counts);
        self.inner.spawn(
            name,
            Box::new(move || {
                let before = current_counts();
                job();
                let made = current_counts().since(&before);
                counts
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(made);
            }),
        )
    }

    fn parallelism(&self) -> u32 {
        self.inner.parallelism()
    }

    fn has_threads(&self) -> bool {
        self.inner.has_threads()
    }
}

/// One frame of scheduler use: a loop, a per-index loop and a scope.
fn frame(scheduler: &dyn IScheduler, sum: &AtomicU64) {
    let add = |value: u64| {
        sum.fetch_add(value, Ordering::Relaxed);
    };
    let loop_result =
        scheduler.parallel_for(1000, 7, &|range| add(u64::from(range.end - range.start)));
    let each_result = scheduler.for_each(100, 1, &|index| add(u64::from(index)));
    let first = || add(1);
    let second = || add(2);
    let third = || add(3);
    let tasks: [&Task<'_>; 3] = [&first, &second, &third];
    let scope_result = scheduler.run_scope(&tasks);
    assert_eq!(
        (loop_result, each_result, scope_result),
        (Ok(()), Ok(()), Ok(()))
    );
}

const FRAME_SUM: u64 = 1000 + 4950 + 6;

/// Allocator calls on the calling thread over `FRAMES` frames after one warm-up frame.
fn caller_calls(scheduler: &dyn IScheduler) -> u64 {
    let sum = AtomicU64::new(0);
    frame(scheduler, &sum);
    let ((), counts) = count_allocations(|| {
        for _ in 0..FRAMES {
            frame(scheduler, &sum);
        }
    });
    assert_eq!(sum.load(Ordering::Relaxed), (FRAMES as u64 + 1) * FRAME_SUM);
    counts.calls()
}

/// Builds a pool, runs the frames, lets the workers park, runs more frames, drops the pool and
/// returns the caller's calls and the counts of every worker thread.
fn measure(workers: u32, policy: WaitPolicy) -> (u64, Vec<AllocCounts>) {
    let threads = MeasuredThreads::new();
    let pool = WorkStealingPool::with_policy(workers, policy, &threads)
        .unwrap_or_else(|e| panic!("a pool of {workers} workers: {e}"));
    let first = caller_calls(&pool);
    std::thread::sleep(Duration::from_millis(20)); // every worker parks
    let second = caller_calls(&pool);
    drop(pool);
    (first + second, threads.worker_counts())
}

#[test]
fn the_counter_sees_an_allocation() {
    let ((), counts) = count_allocations(|| drop(std::hint::black_box(vec![1_u8; 16])));
    assert!(
        counts.calls() >= 1,
        "the counting allocator is not installed"
    );
}

fn assert_no_allocation(workers: u32, policy: WaitPolicy) {
    let (caller, workers_counts) = measure(workers, policy);
    assert_eq!(caller, 0, "caller thread, {workers} workers");
    assert_eq!(workers_counts.len(), workers as usize - 1, "worker reports");
    for counts in workers_counts {
        assert_eq!(
            (counts.allocations, counts.reallocations),
            (0, 0),
            "worker thread, {workers} workers: {counts:?}"
        );
    }
}

#[test]
fn a_one_worker_pool_does_not_allocate_per_call() {
    assert_no_allocation(1, WaitPolicy::DEFAULT);
}

#[test]
fn a_parking_pool_does_not_allocate_per_call() {
    for workers in [2, 3, 4, 7] {
        assert_no_allocation(workers, WaitPolicy::PARK);
    }
}

#[test]
fn a_spinning_pool_does_not_allocate_per_call() {
    for workers in [2, 3, 4, 7] {
        assert_no_allocation(workers, WaitPolicy::DEFAULT);
    }
}
