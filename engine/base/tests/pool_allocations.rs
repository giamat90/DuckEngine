//! The pool makes no allocator call after construction.

use base::alloc::{CountingAllocator, count_allocations};
use base::pool::{Handle, Pool};
use std::alloc::System;
use std::hint::black_box;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

const CAPACITY: u32 = 64;

type Handles = [Option<Handle<u64>>; CAPACITY as usize];

fn fill(pool: &mut Pool<u64>, handles: &mut Handles, round: u64) {
    for (value, slot) in (0_u64..).zip(handles.iter_mut()) {
        *slot = pool.insert(black_box(value + round)).ok();
    }
    assert!(pool.insert(black_box(0)).is_err());
}

fn drain(pool: &mut Pool<u64>, handles: &mut Handles, round: u64) {
    for (value, slot) in (0_u64..).zip(handles.iter_mut()) {
        let stored = slot.and_then(|handle| pool.get(handle).copied());
        assert_eq!(stored, Some(value + round));
        assert!(slot.take().and_then(|handle| pool.remove(handle)).is_some());
    }
}

#[test]
fn building_a_pool_is_two_allocations_slots_and_free_list() {
    let (pool, counts) = count_allocations(|| Pool::<u64>::with_capacity(128));
    assert_eq!(counts.allocations, 2);
    assert_eq!(counts.reallocations, 0);
    drop(pool);
}

#[test]
fn insert_get_remove_and_a_full_pool_make_no_allocator_call() {
    let mut pool = Pool::<u64>::with_capacity(CAPACITY);
    let mut handles: Handles = [None; CAPACITY as usize];
    let ((), counts) = count_allocations(|| {
        for round in 0..4_u64 {
            fill(&mut pool, &mut handles, round);
            drain(&mut pool, &mut handles, round);
        }
    });
    assert_eq!(counts.calls(), 0);
}

#[test]
fn dropping_a_pool_is_two_deallocations() {
    let pool = Pool::<u64>::with_capacity(16);
    let ((), counts) = count_allocations(|| drop(pool));
    assert_eq!(counts.deallocations, 2);
    assert_eq!(counts.allocations, 0);
}
