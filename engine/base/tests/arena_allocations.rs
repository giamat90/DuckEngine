//! The arena makes no allocator call after construction.

use base::alloc::{CountingAllocator, count_allocations};
use base::arena::Arena;
use std::alloc::System;
use std::hint::black_box;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn building_an_arena_is_one_allocation() {
    let (arena, counts) = count_allocations(|| Arena::with_capacity(1_024));
    assert_eq!(counts.allocations, 1);
    assert_eq!(counts.bytes_requested, 1_024);
    drop(arena);
}

#[test]
fn allocating_filling_and_resetting_make_no_allocator_call() {
    let mut arena = Arena::with_capacity(4_096);
    let ((), counts) = count_allocations(|| {
        for round in 0..3_u32 {
            let value = arena.alloc(black_box(round));
            assert!(value.is_some());
            let squares = arena.alloc_slice_with(16, |index| black_box(index * index));
            assert!(squares.is_some());
            let list = arena.vec_with_capacity::<u64>(8);
            assert!(list.is_some_and(|mut list| list.push(1).is_ok()));
            assert!(arena.alloc([0_u8; 8_192]).is_none());
            arena.reset();
        }
    });
    assert_eq!(counts.calls(), 0);
}

#[test]
fn dropping_an_arena_is_one_deallocation() {
    let arena = Arena::with_capacity(64);
    let ((), counts) = count_allocations(|| drop(arena));
    assert_eq!(counts.deallocations, 1);
    assert_eq!(counts.allocations, 0);
}
