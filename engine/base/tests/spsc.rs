//! The single-producer single-consumer queue: order, capacity, drops, and two real threads.

use base::spsc::{Consumer, Producer, channel};
use std::sync::Arc;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn push_blocking<T: Send>(producer: &mut Producer<T>, value: T) {
    let mut pending = value;
    while let Err(back) = producer.push(pending) {
        pending = back;
        std::thread::yield_now();
    }
}

#[test]
fn the_capacity_is_rounded_up_to_a_power_of_two() {
    for (asked, got) in [(1_u32, 1_usize), (2, 2), (3, 4), (5, 8), (1000, 1024)] {
        let Some((producer, _consumer)) = channel::<u32>(asked) else {
            panic!("capacity {asked} refused");
        };
        assert_eq!(producer.capacity(), got, "asked {asked}");
    }
}

#[test]
fn a_capacity_of_zero_or_above_the_limit_is_refused() {
    assert!(channel::<u32>(0).is_none());
    assert!(channel::<u32>(base::spsc::MAX_CAPACITY + 1).is_none());
    assert!(channel::<u32>(u32::MAX).is_none());
}

#[test]
fn the_largest_capacity_is_accepted() {
    // A zero-sized value needs no memory, so the largest queue can be built without much
    // memory, but it still takes seconds to walk 2^30 slots; Miri is too slow for that.
    if cfg!(miri) {
        return;
    }
    assert!(channel::<()>(base::spsc::MAX_CAPACITY).is_some());
}

#[test]
fn values_come_out_in_the_order_they_went_in() -> TestResult {
    let (mut producer, mut consumer) = channel::<u32>(8).ok_or("capacity refused")?;
    for value in 0..5 {
        assert_eq!(producer.push(value), Ok(()));
    }
    for value in 0..5 {
        assert_eq!(consumer.pop(), Some(value));
    }
    assert_eq!(consumer.pop(), None);
    Ok(())
}

#[test]
fn an_empty_queue_pops_nothing_and_reports_empty() -> TestResult {
    let (_producer, mut consumer) = channel::<String>(4).ok_or("capacity refused")?;
    assert!(consumer.is_empty());
    assert_eq!(consumer.pop(), None);
    Ok(())
}

#[test]
fn a_full_queue_gives_the_value_back() -> TestResult {
    let (mut producer, mut consumer) = channel::<u32>(4).ok_or("capacity refused")?;
    for value in 0..4 {
        assert_eq!(producer.push(value), Ok(()));
    }
    assert!(producer.is_full());
    assert_eq!(producer.push(99), Err(99));
    assert_eq!(consumer.pop(), Some(0));
    assert!(!producer.is_full());
    assert_eq!(producer.push(99), Ok(()));
    assert_eq!(producer.push(100), Err(100));
    Ok(())
}

#[test]
fn the_indices_wrap_around_many_times() -> TestResult {
    let (mut producer, mut consumer) = channel::<u32>(4).ok_or("capacity refused")?;
    for value in 0..1000_u32 {
        assert_eq!(producer.push(value), Ok(()));
        assert_eq!(producer.push(value + 5000), Ok(()));
        assert_eq!(consumer.pop(), Some(value));
        assert_eq!(consumer.pop(), Some(value + 5000));
    }
    Ok(())
}

#[test]
fn values_that_need_dropping_are_moved_not_copied() -> TestResult {
    let (mut producer, mut consumer) = channel::<String>(2).ok_or("capacity refused")?;
    assert_eq!(producer.push("one".to_string()), Ok(()));
    assert_eq!(producer.push("two".to_string()), Ok(()));
    assert_eq!(producer.push("three".to_string()), Err("three".to_string()));
    assert_eq!(consumer.pop().as_deref(), Some("one"));
    assert_eq!(consumer.pop().as_deref(), Some("two"));
    Ok(())
}

#[test]
fn values_left_in_the_queue_are_dropped_exactly_once() -> TestResult {
    let token = Arc::new(());
    {
        let (mut producer, mut consumer) = channel::<Arc<()>>(4).ok_or("capacity refused")?;
        for _ in 0..3 {
            assert!(producer.push(Arc::clone(&token)).is_ok());
        }
        assert_eq!(Arc::strong_count(&token), 4);
        drop(consumer.pop());
        assert_eq!(Arc::strong_count(&token), 3);
    }
    assert_eq!(Arc::strong_count(&token), 1);
    Ok(())
}

#[test]
fn either_end_can_be_dropped_first() -> TestResult {
    let token = Arc::new(());
    let (mut producer, consumer) = channel::<Arc<()>>(4).ok_or("capacity refused")?;
    assert!(producer.push(Arc::clone(&token)).is_ok());
    drop(producer);
    assert_eq!(Arc::strong_count(&token), 2);
    drop(consumer);
    assert_eq!(Arc::strong_count(&token), 1);
    Ok(())
}

#[test]
fn the_ends_can_move_to_other_threads() {
    fn is_send<T: Send>() {}
    is_send::<Producer<Arc<u32>>>();
    is_send::<Consumer<Arc<u32>>>();
}

fn message_count() -> u64 {
    if cfg!(miri) { 300 } else { 10_000_000 }
}

#[test]
fn ten_million_messages_arrive_in_order_across_two_threads() -> TestResult {
    let total = message_count();
    let (mut producer, mut consumer) = channel::<u64>(64).ok_or("capacity refused")?;
    let sender = std::thread::spawn(move || {
        for value in 0..total {
            push_blocking(&mut producer, value);
        }
    });
    let mut expected = 0_u64;
    while expected < total {
        match consumer.pop() {
            Some(value) => {
                assert_eq!(value, expected);
                expected += 1;
            }
            None => std::thread::yield_now(),
        }
    }
    sender.join().map_err(|_| "sender panicked")?;
    assert_eq!(consumer.pop(), None);
    Ok(())
}

#[test]
fn heap_values_cross_threads_without_loss_or_double_drop() -> TestResult {
    let total = if cfg!(miri) { 100 } else { 100_000 };
    let token = Arc::new(());
    let (mut producer, mut consumer) = channel::<(u64, Arc<()>)>(16).ok_or("capacity refused")?;
    let for_sender = Arc::clone(&token);
    let sender = std::thread::spawn(move || {
        for value in 0..total {
            push_blocking(&mut producer, (value, Arc::clone(&for_sender)));
        }
    });
    let mut expected = 0_u64;
    while expected < total {
        match consumer.pop() {
            Some((value, _arc)) => {
                assert_eq!(value, expected);
                expected += 1;
            }
            None => std::thread::yield_now(),
        }
    }
    sender.join().map_err(|_| "sender panicked")?;
    assert_eq!(Arc::strong_count(&token), 1);
    Ok(())
}
