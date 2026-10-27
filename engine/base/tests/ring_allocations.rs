//! The ring makes no allocator call after construction.

use base::alloc::{CountingAllocator, count_allocations};
use base::ring::Ring;
use std::alloc::System;
use std::hint::black_box;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

const CAPACITY: u32 = 50;

fn cycle(ring: &mut Ring<u64>, round: u64) {
    for v in 0..u64::from(CAPACITY) {
        assert!(ring.push(black_box(v + round)).is_ok());
    }
    assert!(ring.push(black_box(0)).is_err());
    for v in 0..u64::from(CAPACITY) / 2 {
        assert_eq!(ring.pop(), Some(v + round));
    }
    assert_eq!(ring.iter().count(), CAPACITY as usize / 2);
    ring.clear();
}

#[test]
fn building_a_ring_is_one_allocation() {
    let (ring, counts) = count_allocations(|| Ring::<u64>::with_capacity(128));
    assert_eq!(counts.allocations, 1);
    assert_eq!(counts.reallocations, 0);
    drop(ring);
}

#[test]
fn push_pop_peek_iterate_clear_and_a_full_ring_make_no_allocator_call() {
    let mut ring = Ring::<u64>::with_capacity(CAPACITY);
    let ((), counts) = count_allocations(|| {
        for round in 0..4_u64 {
            cycle(&mut ring, round);
        }
    });
    assert_eq!(counts.calls(), 0);
}

#[test]
fn dropping_a_ring_is_one_deallocation() {
    let ring = Ring::<u64>::with_capacity(16);
    let ((), counts) = count_allocations(|| drop(ring));
    assert_eq!(counts.deallocations, 1);
    assert_eq!(counts.allocations, 0);
}
