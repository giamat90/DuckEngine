//! The counting allocator installed as the global allocator of this test binary.

use base::alloc::{AllocCounts, CountingAllocator, count_allocations, current_counts};
use base::log::{Level, Logger, MemorySink, NullSink};
use std::alloc::System;
use std::hint::black_box;
use std::sync::Barrier;
use std::thread;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn code_that_does_not_allocate_counts_nothing() {
    let (sum, counts) = count_allocations(|| (0..100_u32).sum::<u32>());
    assert_eq!(sum, 4_950);
    assert_eq!(counts.calls(), 0);
}

#[test]
fn a_box_is_one_allocation_of_its_size_and_one_deallocation() {
    let ((), counts) = count_allocations(|| drop(black_box(Box::new(7_u64))));
    assert_eq!(counts.allocations, 1);
    assert_eq!(counts.deallocations, 1);
    assert_eq!(counts.bytes_requested, 8);
    assert_eq!(counts.reallocations, 0);
}

#[test]
fn freeing_a_block_made_earlier_counts_only_the_deallocation() {
    let block = black_box(Box::new([0_u8; 32]));
    let ((), counts) = count_allocations(|| drop(block));
    assert_eq!(counts.allocations, 0);
    assert_eq!(counts.deallocations, 1);
}

#[test]
fn pushing_within_reserved_capacity_makes_no_allocator_call() {
    let mut list: Vec<u32> = Vec::with_capacity(16);
    let ((), counts) = count_allocations(|| {
        for value in 0..16 {
            list.push(black_box(value));
        }
    });
    assert_eq!(counts.calls(), 0);
}

#[test]
fn growing_a_vec_counts_one_allocation_then_reallocations() {
    let ((), counts) = count_allocations(|| {
        let mut list: Vec<u32> = Vec::new();
        for value in 0..5 {
            list.push(black_box(value));
        }
        drop(list);
    });
    // Today std grows 0 -> 4 (allocate) -> 8 (reallocate); that growth policy is a std
    // implementation detail, so only its shape is asserted, not the byte counts.
    assert_eq!(counts.allocations, 1);
    assert!(counts.reallocations >= 1);
    assert_eq!(counts.deallocations, 1);
}

#[test]
fn vec_of_zeros_counts_one_allocation_of_its_size() {
    let ((), counts) = count_allocations(|| drop(black_box(vec![0_u8; 64])));
    assert_eq!(counts.allocations, 1);
    assert_eq!(counts.bytes_requested, 64);
}

#[test]
fn measurements_nest() {
    let (inner, outer) = count_allocations(|| {
        let first = black_box(Box::new(1_u32));
        let ((), inner) = count_allocations(|| drop(black_box(Box::new(2_u32))));
        drop(first);
        inner
    });
    assert_eq!(inner.allocations, 1);
    assert_eq!(outer.allocations, 2);
}

#[test]
fn current_counts_only_grow() {
    let before = current_counts();
    drop(black_box(Box::new(3_u8)));
    let after = current_counts();
    assert_eq!(after.since(&before).allocations, 1);
}

#[test]
fn logging_to_the_null_sink_makes_no_allocator_call() {
    let mut log = Logger::new(NullSink, Level::Trace);
    let ((), counts) = count_allocations(|| {
        for frame in 0..100_u32 {
            log.log(Level::Info, "frame", format_args!("frame {frame}"));
        }
    });
    assert_eq!(counts.calls(), 0);
}

#[test]
fn logging_to_a_memory_sink_does_allocate() {
    let mut log = Logger::new(MemorySink::new(), Level::Trace);
    let ((), counts) = count_allocations(|| {
        log.log(Level::Info, "frame", format_args!("frame {}", 1));
    });
    assert!(counts.allocations > 0);
}

/// Allocates `blocks` one-byte boxes in a `Vec` with reserved capacity (so exactly `blocks + 1`
/// allocations), grows the `Vec` by one slot (one reallocation), waits at `meeting` so that two threads are inside their measurement at the same
/// time, frees everything and returns what this thread's counters saw.
fn allocate_and_meet(blocks: usize, meeting: &Barrier) -> AllocCounts {
    let ((), counts) = count_allocations(|| {
        let mut kept: Vec<Box<u8>> = Vec::with_capacity(blocks);
        for _ in 0..blocks {
            kept.push(black_box(Box::new(1_u8)));
        }
        kept.reserve_exact(1);
        meeting.wait();
        drop(kept);
    });
    counts
}

/// What a thread that ran `allocate_and_meet(blocks, ..)` must have counted, all four counters.
fn expected_counts(blocks: usize) -> AllocCounts {
    let blocks = blocks as u64;
    let slot = size_of::<Box<u8>>() as u64;
    AllocCounts {
        allocations: blocks + 1,
        reallocations: 1,
        deallocations: blocks + 1,
        // the boxes, the list, and the list again at its grown size
        bytes_requested: blocks + blocks * slot + (blocks + 1) * slot,
    }
}

#[test]
fn each_thread_counts_only_its_own_allocations_while_both_run_at_once() {
    const MAIN_BLOCKS: usize = 3;
    const WORKER_BLOCKS: usize = 50;
    let meeting = Barrier::new(2);
    let (main_seen, worker_seen) = thread::scope(|scope| {
        let worker = scope.spawn(|| allocate_and_meet(WORKER_BLOCKS, &meeting));
        let main_seen = allocate_and_meet(MAIN_BLOCKS, &meeting);
        (main_seen, worker.join().unwrap_or_default())
    });
    assert_eq!(main_seen, expected_counts(MAIN_BLOCKS));
    assert_eq!(worker_seen, expected_counts(WORKER_BLOCKS));
}

#[test]
fn a_new_thread_starts_counting_from_zero() {
    // This thread has allocated before, so a counter shared with the new thread would not be zero.
    let _earlier = black_box(Box::new(0_u8));
    let seen = thread::scope(|scope| scope.spawn(current_counts).join().unwrap_or_default());
    // Deallocations are not asserted: std hands the new thread blocks made by its parent and the
    // new thread frees them, which is counted there (and is std's business, not ours).
    assert_eq!(
        (seen.allocations, seen.reallocations, seen.bytes_requested),
        (0, 0, 0)
    );
}
