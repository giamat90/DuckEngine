//! `FixedStep::advance` makes no allocator call.

use base::alloc::{CountingAllocator, count_allocations};
use frame::FixedStep;
use std::alloc::System;
use std::hint::black_box;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn advance_makes_no_allocator_call() {
    let mut f = FixedStep::new(16, 3).unwrap_or_else(|_| panic!("valid"));
    let ((), counts) = count_allocations(|| {
        for frame in 0..1_000_u64 {
            black_box(f.advance(black_box(frame * 7)));
        }
    });
    assert_eq!(counts.calls(), 0);
}
