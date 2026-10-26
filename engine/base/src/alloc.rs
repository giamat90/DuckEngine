//! A counting allocator with per-thread counters, for tests that assert "this
//! code made no allocator calls".
//!
//! The counters are per thread on purpose: shared atomic counters made a
//! single thread 29% slower and eight threads 5.9 times slower in the
//! allocator spike (wiki/spikes/allocator-spike.md, S7).
#![expect(
    unsafe_code,
    reason = "the one module of this crate that may use unsafe; every block has a SAFETY comment"
)]

use core::alloc::{GlobalAlloc, Layout};
use core::cell::Cell;

thread_local! {
    // `const` initialiser and no destructor: reading it never allocates and never
    // registers a thread-exit hook, so it is safe to use from inside the allocator.
    static COUNTS: Cell<AllocCounts> = const { Cell::new(AllocCounts::ZERO) };
}

/// Allocator calls made by one thread. Calls are counted when made, including
/// requests the allocator then fails (null result). All counters saturate:
/// the allocator must never panic, so an overflow stops at `u64::MAX`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AllocCounts {
    /// Calls to `alloc` and `alloc_zeroed`.
    pub allocations: u64,
    /// Calls to `realloc` (a growing `Vec` makes these).
    pub reallocations: u64,
    /// Calls to `dealloc`.
    pub deallocations: u64,
    /// Bytes requested by allocations and by the new size of reallocations.
    pub bytes_requested: u64,
}

impl AllocCounts {
    const ZERO: Self = Self {
        allocations: 0,
        reallocations: 0,
        deallocations: 0,
        bytes_requested: 0,
    };

    fn add_allocation(&mut self, size: usize) {
        self.allocations = self.allocations.saturating_add(1);
        self.bytes_requested = self.bytes_requested.saturating_add(bytes(size));
    }

    fn add_reallocation(&mut self, new_size: usize) {
        self.reallocations = self.reallocations.saturating_add(1);
        self.bytes_requested = self.bytes_requested.saturating_add(bytes(new_size));
    }

    fn add_deallocation(&mut self) {
        self.deallocations = self.deallocations.saturating_add(1);
    }

    /// All allocator calls together.
    #[must_use]
    pub fn calls(&self) -> u64 {
        self.allocations
            .saturating_add(self.reallocations)
            .saturating_add(self.deallocations)
    }

    /// The calls made after `earlier` was taken.
    #[must_use]
    pub fn since(&self, earlier: &Self) -> Self {
        Self {
            allocations: self.allocations.saturating_sub(earlier.allocations),
            reallocations: self.reallocations.saturating_sub(earlier.reallocations),
            deallocations: self.deallocations.saturating_sub(earlier.deallocations),
            bytes_requested: self.bytes_requested.saturating_sub(earlier.bytes_requested),
        }
    }
}

fn record(update: impl FnOnce(&mut AllocCounts)) {
    COUNTS.with(|cell| {
        let mut counts = cell.get();
        update(&mut counts);
        cell.set(counts);
    });
}

fn bytes(size: usize) -> u64 {
    u64::try_from(size).unwrap_or(u64::MAX)
}

/// Wraps an allocator and counts its calls per thread. Install it with
/// `#[global_allocator]` in a test binary or an application.
#[derive(Debug, Default)]
pub struct CountingAllocator<A> {
    inner: A,
}

impl<A> CountingAllocator<A> {
    /// Wraps `inner`.
    #[must_use]
    pub const fn new(inner: A) -> Self {
        Self { inner }
    }
}

// SAFETY: every method forwards to the wrapped allocator with the arguments it
// received, so the contract of `GlobalAlloc` holds exactly when it holds for `A`.
// Counting touches only a thread-local `Cell` and never allocates or panics. That
// relies on native thread-local storage with a `const` initialiser, which holds on
// every supported target (Windows MSVC, Linux); on a target that emulates
// thread-locals with OS keys the first access could allocate and re-enter.
unsafe impl<A: GlobalAlloc> GlobalAlloc for CountingAllocator<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(|counts| counts.add_allocation(layout.size()));
        // SAFETY: the caller upholds the `GlobalAlloc::alloc` contract for `layout`.
        unsafe { self.inner.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        record(AllocCounts::add_deallocation);
        // SAFETY: the caller guarantees `ptr` came from this allocator with `layout`,
        // and every block of this allocator comes from `inner`.
        unsafe { self.inner.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(|counts| counts.add_allocation(layout.size()));
        // SAFETY: the caller upholds the `GlobalAlloc::alloc_zeroed` contract for `layout`.
        unsafe { self.inner.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record(|counts| counts.add_reallocation(new_size));
        // SAFETY: the caller guarantees `ptr` came from this allocator with `layout`
        // and `new_size` is valid for `layout.align()`.
        unsafe { self.inner.realloc(ptr, layout, new_size) }
    }
}

/// The calls the current thread has made so far (zero unless a
/// [`CountingAllocator`] is the global allocator).
#[must_use]
pub fn current_counts() -> AllocCounts {
    COUNTS.with(Cell::get)
}

/// Runs `f` and returns its result with the allocator calls the current thread
/// made while it ran. Other threads are not counted.
pub fn count_allocations<R>(f: impl FnOnce() -> R) -> (R, AllocCounts) {
    let before = current_counts();
    let result = f();
    (result, current_counts().since(&before))
}

#[cfg(test)]
mod tests {
    use super::{AllocCounts, CountingAllocator, current_counts};
    use core::alloc::{GlobalAlloc, Layout};
    use std::alloc::System;

    const LATER: AllocCounts = AllocCounts {
        allocations: 10,
        reallocations: 20,
        deallocations: 30,
        bytes_requested: 400,
    };
    const EARLIER: AllocCounts = AllocCounts {
        allocations: 1,
        reallocations: 2,
        deallocations: 3,
        bytes_requested: 4,
    };

    #[test]
    fn calls_add_allocations_reallocations_and_deallocations() {
        assert_eq!(LATER.calls(), 60);
        assert_eq!(EARLIER.calls(), 6);
    }

    #[test]
    fn calls_ignore_the_byte_count() {
        let only_bytes = AllocCounts {
            bytes_requested: 99,
            ..AllocCounts::default()
        };
        assert_eq!(only_bytes.calls(), 0);
    }

    #[test]
    fn since_subtracts_every_field() {
        let expected = AllocCounts {
            allocations: 9,
            reallocations: 18,
            deallocations: 27,
            bytes_requested: 396,
        };
        assert_eq!(LATER.since(&EARLIER), expected);
    }

    #[test]
    fn since_an_equal_snapshot_is_zero() {
        assert_eq!(LATER.since(&LATER), AllocCounts::default());
    }

    #[test]
    fn since_a_later_snapshot_stops_at_zero_instead_of_wrapping() {
        assert_eq!(EARLIER.since(&LATER), AllocCounts::default());
    }

    const FULL: AllocCounts = AllocCounts {
        allocations: u64::MAX,
        reallocations: u64::MAX,
        deallocations: u64::MAX,
        bytes_requested: u64::MAX,
    };

    #[test]
    fn counters_saturate_instead_of_overflowing() {
        // The allocator must never panic, so a full counter stops at the maximum.
        let mut counts = FULL;
        counts.add_allocation(8);
        counts.add_reallocation(8);
        counts.add_deallocation();
        assert_eq!(counts, FULL);
    }

    #[test]
    fn byte_count_saturates_on_the_request_that_overflows_it() {
        let mut counts = AllocCounts {
            bytes_requested: u64::MAX - 4,
            ..AllocCounts::default()
        };
        counts.add_allocation(8);
        assert_eq!(counts.bytes_requested, u64::MAX);
        assert_eq!(counts.allocations, 1);
    }

    #[test]
    fn calls_saturate_instead_of_overflowing() {
        assert_eq!(FULL.calls(), u64::MAX);
    }

    #[test]
    fn direct_calls_are_forwarded_and_counted_on_this_thread() {
        let allocator = CountingAllocator::new(System);
        let small = Layout::new::<[u64; 3]>();
        let before = current_counts();
        // SAFETY: `small` has a non-zero size; the block is written inside its 24
        // bytes, reallocated with its own layout, and freed with the layout it
        // was last allocated with.
        unsafe {
            let block = allocator.alloc_zeroed(small);
            assert!(!block.is_null());
            assert_eq!(block.read(), 0);
            block.write(7);
            let grown = allocator.realloc(block, small, 48);
            assert!(!grown.is_null());
            assert_eq!(grown.read(), 7);
            let big = Layout::new::<[u64; 6]>();
            allocator.dealloc(grown, big);
        }
        let counts = current_counts().since(&before);
        assert_eq!(counts.allocations, 1);
        assert_eq!(counts.reallocations, 1);
        assert_eq!(counts.deallocations, 1);
        assert_eq!(counts.bytes_requested, 24 + 48);
    }
}
