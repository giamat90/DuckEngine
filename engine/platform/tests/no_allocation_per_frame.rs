//! The per-frame calls of the clock and thread ports make no allocator call.

use base::alloc::{CountingAllocator, count_allocations};
use platform::clock::Ticks;
use platform::platform::Platform;
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

fn frame(platform: &Platform) -> u64 {
    let now = platform.clock.now();
    let threads = platform.threads.parallelism();
    now.as_nanos() + u64::from(threads) + u64::from(platform.caps().has_threads)
}

#[test]
fn the_std_platform_frame_calls_do_not_allocate() {
    let platform = Platform::std();
    let (_, counts) = count_allocations(|| (0..100).map(|_| frame(&platform)).sum::<u64>());
    assert_eq!(counts.calls(), 0);
}

#[test]
fn the_headless_platform_frame_calls_do_not_allocate() {
    let (platform, clock) = Platform::headless();
    let ((), counts) = count_allocations(|| {
        for _ in 0..100 {
            clock.advance(Ticks::from_millis(16));
            platform.clock.sleep(Ticks::from_nanos(1));
            frame(&platform);
        }
    });
    assert_eq!(counts.calls(), 0);
}
