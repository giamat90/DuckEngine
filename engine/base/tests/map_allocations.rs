//! The map makes no allocator call after construction.

use base::alloc::{CountingAllocator, count_allocations};
use base::map::DenseMap;
use std::alloc::System;
use std::hint::black_box;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

const CAPACITY: u32 = 64;

fn fill(map: &mut DenseMap<u64, u64>, round: u64) {
    for key in 0..u64::from(CAPACITY) {
        assert!(
            map.insert(black_box(key << 32), black_box(key + round))
                .is_ok()
        );
    }
    assert!(map.insert(black_box(u64::MAX), black_box(0)).is_err());
}

fn drain(map: &mut DenseMap<u64, u64>, round: u64) {
    for key in 0..u64::from(CAPACITY) {
        assert_eq!(map.get(&(key << 32)), Some(&(key + round)));
    }
    assert_eq!(map.iter().count(), CAPACITY as usize);
    for key in (0..u64::from(CAPACITY)).rev() {
        assert_eq!(map.remove(&(key << 32)), Some(key + round));
    }
}

#[test]
fn building_a_map_is_two_allocations_entries_and_table() {
    let (map, counts) = count_allocations(|| DenseMap::<u64, u64>::with_capacity(128));
    assert_eq!(counts.allocations, 2);
    assert_eq!(counts.reallocations, 0);
    drop(map);
}

#[test]
fn insert_get_remove_iterate_clear_and_a_full_map_make_no_allocator_call() {
    let mut map = DenseMap::<u64, u64>::with_capacity(CAPACITY);
    let ((), counts) = count_allocations(|| {
        for round in 0..4_u64 {
            fill(&mut map, round);
            drain(&mut map, round);
            fill(&mut map, round);
            map.clear();
        }
    });
    assert_eq!(counts.calls(), 0);
}

#[test]
fn dropping_a_map_is_two_deallocations() {
    let map = DenseMap::<u64, u64>::with_capacity(16);
    let ((), counts) = count_allocations(|| drop(map));
    assert_eq!(counts.deallocations, 2);
    assert_eq!(counts.allocations, 0);
}
