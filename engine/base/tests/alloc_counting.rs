//! The counting allocator installed as the global allocator of this test binary.

use base::alloc::{CountingAllocator, count_allocations, current_counts};
use base::log::{Level, Logger, MemorySink, NullSink};
use std::alloc::System;
use std::hint::black_box;

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
