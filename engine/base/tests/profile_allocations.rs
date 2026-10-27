//! A scope makes no allocator call, whether or not a sink is installed.
//! The sink here only counts, so any allocation measured comes from the hook itself.

use base::alloc::{CountingAllocator, count_allocations};
use base::profile::{ProfileSink, install_sink, scope};
use std::alloc::System;
use std::sync::atomic::{AtomicUsize, Ordering};

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

struct Counter {
    enters: AtomicUsize,
    exits: AtomicUsize,
}

impl ProfileSink for Counter {
    fn enter(&self, _name: &'static str) {
        self.enters.fetch_add(1, Ordering::Relaxed);
    }

    fn exit(&self, _name: &'static str) {
        self.exits.fetch_add(1, Ordering::Relaxed);
    }
}

static COUNTER: Counter = Counter {
    enters: AtomicUsize::new(0),
    exits: AtomicUsize::new(0),
};

fn hundred_scopes() {
    for _ in 0..100 {
        let _outer = scope("outer");
        let _inner = scope("inner");
    }
}

#[test]
fn scopes_make_no_allocator_call_without_and_with_a_sink() {
    let ((), without) = count_allocations(hundred_scopes);
    assert_eq!(without.calls(), 0);
    assert!(install_sink(&COUNTER).is_ok());
    let ((), with) = count_allocations(hundred_scopes);
    assert_eq!(with.calls(), 0);
    assert_eq!(COUNTER.enters.load(Ordering::Relaxed), 200);
    assert_eq!(COUNTER.exits.load(Ordering::Relaxed), 200);
}
