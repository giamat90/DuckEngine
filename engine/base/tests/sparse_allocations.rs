//! The sparse set makes no allocator call after construction.

use base::alloc::{CountingAllocator, count_allocations};
use base::sparse::SparseSet;
use std::alloc::System;
use std::hint::black_box;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

const CAPACITY: u32 = 64;
const ID_BOUND: u32 = 4096;

fn fill(set: &mut SparseSet<u64>, round: u64) {
    for id in 0..CAPACITY {
        assert!(
            set.insert(black_box(id * 3), black_box(u64::from(id) + round))
                .is_ok()
        );
    }
    assert!(set.insert(black_box(1), black_box(0)).is_err());
    assert!(set.insert(black_box(ID_BOUND), black_box(0)).is_err());
}

fn drain(set: &mut SparseSet<u64>, round: u64) {
    for id in 0..CAPACITY {
        assert_eq!(set.get(id * 3), Some(&(u64::from(id) + round)));
    }
    assert_eq!(set.iter().count(), CAPACITY as usize);
    for id in (0..CAPACITY).rev() {
        assert_eq!(set.remove(id * 3), Some(u64::from(id) + round));
    }
}

#[test]
fn building_a_set_is_two_allocations_entries_and_table() {
    let (set, counts) = count_allocations(|| SparseSet::<u64>::with_capacity(ID_BOUND, 128));
    assert_eq!(counts.allocations, 2);
    assert_eq!(counts.reallocations, 0);
    drop(set);
}

#[test]
fn insert_get_remove_iterate_clear_and_refusals_make_no_allocator_call() {
    let mut set = SparseSet::<u64>::with_capacity(ID_BOUND, CAPACITY);
    let ((), counts) = count_allocations(|| {
        for round in 0..4_u64 {
            fill(&mut set, round);
            drain(&mut set, round);
            fill(&mut set, round);
            set.clear();
        }
    });
    assert_eq!(counts.calls(), 0);
}

#[test]
fn dropping_a_set_is_two_deallocations() {
    let set = SparseSet::<u64>::with_capacity(ID_BOUND, 16);
    let ((), counts) = count_allocations(|| drop(set));
    assert_eq!(counts.deallocations, 2);
    assert_eq!(counts.allocations, 0);
}
