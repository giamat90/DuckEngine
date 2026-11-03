//! The two deques of the stealing candidates. Items are packed item ranges (`lo << 32 | hi`).
//!
//! Both have a fixed capacity and never allocate after construction. The splitting scheme of
//! [`crate::stealing`] keeps at most one pending half per level, so 64 slots are enough for any
//! `u32` loop; a full deque is reported to the caller, which then runs the range inline.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering, fence};

use crate::sync::lock;

/// Slots per deque (a power of two).
pub const CAPACITY: usize = 64;
/// `CAPACITY - 1`, as the signed index type of the Chase-Lev counters.
const MASK: i64 = 63;

/// A work-stealing deque: the owner pushes and pops at one end, any thread steals at the other.
pub trait Deque: Default + Send + Sync + 'static {
    /// Owner only. False when the deque is full.
    fn push(&self, item: u64) -> bool;
    /// Owner only: the newest item.
    fn pop(&self) -> Option<u64>;
    /// Any thread: the oldest item.
    fn steal(&self) -> Option<u64>;
}

/// Candidate (b): a `VecDeque` behind a `Mutex`.
pub struct MutexDeque {
    items: Mutex<VecDeque<u64>>,
}

impl Default for MutexDeque {
    fn default() -> Self {
        Self {
            items: Mutex::new(VecDeque::with_capacity(CAPACITY)),
        }
    }
}

impl Deque for MutexDeque {
    fn push(&self, item: u64) -> bool {
        let mut items = lock(&self.items);
        if items.len() >= CAPACITY {
            return false;
        }
        items.push_back(item);
        true
    }

    fn pop(&self) -> Option<u64> {
        lock(&self.items).pop_back()
    }

    fn steal(&self) -> Option<u64> {
        lock(&self.items).pop_front()
    }
}

/// Candidate (c): a Chase-Lev deque (Le, Pop, Cohen, Zappa Nardelli 2013, the C11 formulation)
/// whose slots are atomics, so no `unsafe` is needed: the items are plain 64-bit numbers.
/// `top` and `bottom` only grow (as `i64`), the slot is `index & MASK`.
pub struct ChaseLevDeque {
    top: AtomicI64,
    bottom: AtomicI64,
    slots: [AtomicU64; CAPACITY],
}

impl Default for ChaseLevDeque {
    fn default() -> Self {
        Self {
            top: AtomicI64::new(0),
            bottom: AtomicI64::new(0),
            slots: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
}

impl ChaseLevDeque {
    fn slot(&self, index: i64) -> &AtomicU64 {
        // The mask keeps the value in 0..CAPACITY, so the conversion cannot lose anything.
        &self.slots[usize::try_from(index & MASK).unwrap_or(0)]
    }
}

impl Deque for ChaseLevDeque {
    fn push(&self, item: u64) -> bool {
        let bottom = self.bottom.load(Ordering::Relaxed);
        let top = self.top.load(Ordering::Acquire);
        if bottom - top > MASK {
            return false;
        }
        self.slot(bottom).store(item, Ordering::Relaxed);
        fence(Ordering::Release);
        self.bottom.store(bottom + 1, Ordering::Relaxed);
        true
    }

    fn pop(&self) -> Option<u64> {
        let bottom = self.bottom.load(Ordering::Relaxed) - 1;
        self.bottom.store(bottom, Ordering::Relaxed);
        fence(Ordering::SeqCst);
        let top = self.top.load(Ordering::Relaxed);
        if top > bottom {
            self.bottom.store(bottom + 1, Ordering::Relaxed);
            return None;
        }
        let item = self.slot(bottom).load(Ordering::Relaxed);
        if top < bottom {
            return Some(item);
        }
        // The last item: race the thieves for it.
        let won = self
            .top
            .compare_exchange(top, top + 1, Ordering::SeqCst, Ordering::Relaxed)
            .is_ok();
        self.bottom.store(bottom + 1, Ordering::Relaxed);
        won.then_some(item)
    }

    fn steal(&self) -> Option<u64> {
        let mut attempt = self.try_steal();
        while attempt == Steal::Retry {
            attempt = self.try_steal();
        }
        match attempt {
            Steal::Item(item) => Some(item),
            Steal::Empty | Steal::Retry => None,
        }
    }
}

/// The answer of one steal attempt.
#[derive(PartialEq, Eq)]
enum Steal {
    Empty,
    Retry,
    Item(u64),
}

impl ChaseLevDeque {
    fn try_steal(&self) -> Steal {
        let top = self.top.load(Ordering::Acquire);
        fence(Ordering::SeqCst);
        let bottom = self.bottom.load(Ordering::Acquire);
        if top >= bottom {
            return Steal::Empty;
        }
        let item = self.slot(top).load(Ordering::Relaxed);
        let won = self
            .top
            .compare_exchange(top, top + 1, Ordering::SeqCst, Ordering::Relaxed)
            .is_ok();
        if won { Steal::Item(item) } else { Steal::Retry }
    }
}

/// Packs the item range `lo..hi`.
#[must_use]
pub fn pack(lo: u32, hi: u32) -> u64 {
    (u64::from(lo) << 32) | u64::from(hi)
}

/// The item range of a packed item.
#[must_use]
pub fn unpack(item: u64) -> (u32, u32) {
    // The high half is shifted down and the low half masked, so both fit.
    (
        u32::try_from(item >> 32).unwrap_or(u32::MAX),
        u32::try_from(item & u64::from(u32::MAX)).unwrap_or(u32::MAX),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    fn single_thread_semantics<D: Deque>() {
        let deque = D::default();
        assert_eq!(deque.pop(), None);
        assert_eq!(deque.steal(), None);
        assert!(deque.push(1) && deque.push(2) && deque.push(3));
        assert_eq!(deque.pop(), Some(3), "the owner takes the newest");
        assert_eq!(deque.steal(), Some(1), "a thief takes the oldest");
        assert_eq!(deque.pop(), Some(2));
        assert_eq!(deque.pop(), None);
        assert!((0..CAPACITY as u64).all(|item| deque.push(item)));
        assert!(!deque.push(99), "a full deque refuses");
        assert_eq!(deque.steal(), Some(0));
        assert!(deque.push(99), "a steal frees a slot");
    }

    #[test]
    fn the_mutex_deque_has_deque_semantics() {
        single_thread_semantics::<MutexDeque>();
    }

    #[test]
    fn the_lock_free_deque_has_deque_semantics() {
        single_thread_semantics::<ChaseLevDeque>();
    }

    #[test]
    fn the_mask_is_the_capacity_minus_one() {
        assert_eq!(usize::try_from(MASK + 1), Ok(CAPACITY));
    }

    #[test]
    fn packing_round_trips() {
        for (lo, hi) in [(0, 0), (1, 2), (u32::MAX, u32::MAX), (7, u32::MAX)] {
            assert_eq!(unpack(pack(lo, hi)), (lo, hi));
        }
    }

    fn thief<D: Deque>(deque: &D, done: &AtomicBool) -> Vec<u64> {
        let mut taken = Vec::new();
        while !done.load(Ordering::Acquire) {
            taken.extend(deque.steal());
        }
        while let Some(item) = deque.steal() {
            taken.push(item);
        }
        taken
    }

    fn push_and_sometimes_pop<D: Deque>(deque: &D, item: u64, taken: &mut Vec<u64>) {
        while !deque.push(item) {
            taken.extend(deque.pop());
        }
        if item.is_multiple_of(3) {
            taken.extend(deque.pop());
        }
    }

    /// One owner pushes and pops, three thieves steal: every item is taken exactly once.
    fn every_item_is_taken_exactly_once<D: Deque>() {
        const ITEMS: u64 = 20_000;
        let deque = Arc::new(D::default());
        let done = Arc::new(AtomicBool::new(false));
        let thieves: Vec<_> = (0..3)
            .map(|_| {
                let (deque, done) = (Arc::clone(&deque), Arc::clone(&done));
                std::thread::spawn(move || thief(&*deque, &done))
            })
            .collect();
        let mut taken = Vec::new();
        (0..ITEMS).for_each(|item| push_and_sometimes_pop(&*deque, item, &mut taken));
        while let Some(item) = deque.pop() {
            taken.push(item);
        }
        done.store(true, Ordering::Release);
        taken.extend(
            thieves
                .into_iter()
                .flat_map(|t| t.join().unwrap_or_default()),
        );
        taken.sort_unstable();
        assert_eq!(taken, (0..ITEMS).collect::<Vec<_>>());
    }

    #[test]
    fn the_mutex_deque_loses_and_duplicates_nothing_under_stealing() {
        every_item_is_taken_exactly_once::<MutexDeque>();
    }

    #[test]
    fn the_lock_free_deque_loses_and_duplicates_nothing_under_stealing() {
        every_item_is_taken_exactly_once::<ChaseLevDeque>();
    }
}
