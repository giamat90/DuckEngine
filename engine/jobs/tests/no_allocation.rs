//! After construction, the calls of every scheduler make no allocator call on the calling thread
//! (counting allocator with per-thread counters, as the platform and render tests do).

use base::alloc::{CountingAllocator, count_allocations};
use jobs::inline::InlineScheduler;
use jobs::reversed::ReversedScheduler;
use jobs::scheduler::{IScheduler, Task};
use jobs::shuffled::ShuffledScheduler;
use std::alloc::System;
use std::sync::atomic::{AtomicU64, Ordering};

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

/// One frame of scheduler use: a loop, a per-index loop and a scope; returns a checksum so the
/// work cannot be optimised away.
fn frame(scheduler: &dyn IScheduler, sum: &AtomicU64) {
    let add = |value: u64| {
        sum.fetch_add(value, Ordering::Relaxed);
    };
    let loop_result =
        scheduler.parallel_for(100, 7, &|range| add(u64::from(range.end - range.start)));
    let each_result = scheduler.for_each(100, 7, &|index| add(u64::from(index)));
    let first = || add(1);
    let second = || add(2);
    let tasks: [&Task<'_>; 2] = [&first, &second];
    let scope_result = scheduler.run_scope(&tasks);
    assert_eq!(
        (loop_result, each_result, scope_result),
        (Ok(()), Ok(()), Ok(()))
    );
}

fn calls_in_steady_state(scheduler: &dyn IScheduler) -> u64 {
    let sum = AtomicU64::new(0);
    frame(scheduler, &sum); // warm-up
    let ((), counts) = count_allocations(|| {
        for _ in 0..100 {
            frame(scheduler, &sum);
        }
    });
    assert_eq!(sum.load(Ordering::Relaxed), 101 * (100 + 4950 + 3));
    counts.calls()
}

#[test]
fn the_counter_sees_an_allocation() {
    let ((), counts) = count_allocations(|| drop(std::hint::black_box(vec![1_u8; 16])));
    assert!(
        counts.calls() >= 1,
        "the counting allocator is not installed"
    );
}

#[test]
fn the_inline_scheduler_does_not_allocate() {
    assert_eq!(calls_in_steady_state(&InlineScheduler), 0);
}

#[test]
fn the_reversed_scheduler_does_not_allocate() {
    assert_eq!(calls_in_steady_state(&ReversedScheduler), 0);
}

#[test]
fn the_shuffled_scheduler_does_not_allocate_after_construction() {
    assert_eq!(calls_in_steady_state(&ShuffledScheduler::new(3, 32)), 0);
}
