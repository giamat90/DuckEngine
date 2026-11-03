//! The port bounds that keep the later pool possible: schedulers are `Send + Sync`, a body that
//! borrows the caller's data is accepted, and one `&dyn IScheduler` can be used from another
//! thread (a pool is shared that way by the engine's systems).

use jobs::inline::InlineScheduler;
use jobs::reversed::ReversedScheduler;
use jobs::scheduler::IScheduler;
use jobs::shuffled::ShuffledScheduler;
use std::sync::atomic::{AtomicU64, Ordering};

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn every_scheduler_and_the_trait_object_are_send_and_sync() {
    assert_send_sync::<InlineScheduler>();
    assert_send_sync::<ReversedScheduler>();
    assert_send_sync::<ShuffledScheduler>();
    assert_send_sync::<&dyn IScheduler>();
    assert_send_sync::<Box<dyn IScheduler>>();
}

/// Sums 0..100 with a body that borrows `total` from this frame.
fn sum_to_100(scheduler: &dyn IScheduler) -> u64 {
    let total = AtomicU64::new(0);
    let result = scheduler.for_each(100, 8, &|index| {
        total.fetch_add(u64::from(index), Ordering::Relaxed);
    });
    assert_eq!(result, Ok(()));
    total.load(Ordering::Relaxed)
}

#[test]
fn a_scheduler_shared_by_reference_sums_a_loop_on_another_thread() {
    let schedulers: [Box<dyn IScheduler>; 3] = [
        Box::new(InlineScheduler),
        Box::new(ReversedScheduler),
        Box::new(ShuffledScheduler::new(9, 64)),
    ];
    std::thread::scope(|scope| {
        for scheduler in &schedulers {
            scope.spawn(move || assert_eq!(sum_to_100(scheduler.as_ref()), 4950));
        }
    });
}
