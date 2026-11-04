//! The per-worker work-stealing deque: a Chase-Lev deque (Le, Pop, Cohen, Zappa Nardelli 2013,
//! the C11 formulation) of fixed capacity. Its slots are atomics holding plain 64-bit numbers
//! (a packed item range), so the deque needs no `unsafe`; it never allocates after construction.
//!
//! The owner pushes and pops at the bottom, any thread steals at the top. `top` and `bottom` only
//! grow (as `i64`), the slot of an index is `index & MASK`.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering, fence};

/// Slots per deque (a power of two). The splitting scheme of the pool keeps at most one pending
/// half per level of a 32-bit range, so 64 slots are enough for any loop.
pub const CAPACITY: usize = 64;
/// `CAPACITY - 1`, as the signed index type of the counters.
const MASK: i64 = 63;

/// A Chase-Lev deque of packed item ranges.
pub struct Deque {
    top: AtomicI64,
    bottom: AtomicI64,
    slots: [AtomicU64; CAPACITY],
}

impl Deque {
    /// An empty deque.
    #[must_use]
    pub fn new() -> Self {
        Self {
            top: AtomicI64::new(0),
            bottom: AtomicI64::new(0),
            slots: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    fn slot(&self, index: i64) -> &AtomicU64 {
        // The mask keeps the value in 0..CAPACITY, so the conversion cannot lose anything.
        &self.slots[usize::try_from(index & MASK).unwrap_or(0)]
    }

    /// Owner only. False when the deque is full.
    pub fn push(&self, item: u64) -> bool {
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

    /// Owner only: the newest item.
    pub fn pop(&self) -> Option<u64> {
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

    /// Any thread: the oldest item. `None` only when the deque was empty; a lost race against
    /// another thief or the owner is retried.
    pub fn steal(&self) -> Option<u64> {
        let mut attempt = self.try_steal();
        while matches!(attempt, Steal::Retry) {
            attempt = self.try_steal();
        }
        match attempt {
            Steal::Item(item) => Some(item),
            Steal::Empty | Steal::Retry => None,
        }
    }

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

impl Default for Deque {
    fn default() -> Self {
        Self::new()
    }
}

/// The answer of one steal attempt.
enum Steal {
    Empty,
    Retry,
    Item(u64),
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

    #[test]
    fn an_empty_deque_gives_nothing() {
        let deque = Deque::new();
        assert_eq!(deque.pop(), None);
        assert_eq!(deque.steal(), None);
    }

    #[test]
    fn the_owner_takes_the_newest_and_a_thief_the_oldest() {
        let deque = Deque::new();
        assert!(deque.push(1) && deque.push(2) && deque.push(3));
        assert_eq!(deque.pop(), Some(3), "the owner takes the newest");
        assert_eq!(deque.steal(), Some(1), "a thief takes the oldest");
        assert_eq!(deque.pop(), Some(2));
        assert_eq!(deque.pop(), None);
        assert_eq!(deque.steal(), None);
    }

    #[test]
    fn a_full_deque_refuses_and_a_steal_frees_a_slot() {
        let deque = Deque::new();
        assert!((0..CAPACITY as u64).all(|item| deque.push(item)));
        assert!(!deque.push(99), "a full deque refuses");
        assert_eq!(deque.steal(), Some(0));
        assert!(deque.push(99), "a steal frees a slot");
        assert_eq!(deque.pop(), Some(99));
    }

    #[test]
    fn indices_wrap_around_the_ring_many_times() {
        let deque = Deque::new();
        for item in 0..(CAPACITY as u64 * 5) {
            assert!(deque.push(item));
            assert!(deque.push(item + 1000));
            assert_eq!(deque.steal(), Some(item));
            assert_eq!(deque.pop(), Some(item + 1000));
        }
        assert_eq!(deque.pop(), None);
    }

    #[test]
    fn popping_from_empty_leaves_the_deque_usable() {
        let deque = Deque::new();
        assert_eq!(deque.pop(), None);
        assert_eq!(deque.pop(), None);
        assert!(deque.push(7));
        assert_eq!(deque.steal(), Some(7));
    }

    #[test]
    fn the_mask_is_the_capacity_minus_one() {
        assert_eq!(usize::try_from(MASK + 1), Ok(CAPACITY));
    }

    #[test]
    fn packing_round_trips() {
        for (lo, hi) in [
            (0, 0),
            (1, 2),
            (u32::MAX, u32::MAX),
            (7, u32::MAX),
            (u32::MAX, 3),
        ] {
            assert_eq!(unpack(pack(lo, hi)), (lo, hi));
        }
        assert_eq!(pack(1, 2), (1 << 32) | 2);
    }

    fn thief(deque: &Deque, done: &AtomicBool) -> Vec<u64> {
        let mut taken = Vec::new();
        while !done.load(Ordering::Acquire) {
            taken.extend(deque.steal());
        }
        while let Some(item) = deque.steal() {
            taken.push(item);
        }
        taken
    }

    fn push_and_sometimes_pop(deque: &Deque, item: u64, taken: &mut Vec<u64>) {
        while !deque.push(item) {
            taken.extend(deque.pop());
        }
        if item.is_multiple_of(3) {
            taken.extend(deque.pop());
        }
    }

    /// One owner pushes and pops, `thieves` threads steal: every item is taken exactly once.
    fn every_item_is_taken_exactly_once(items: u64, thieves: usize) {
        let deque = Arc::new(Deque::new());
        let done = Arc::new(AtomicBool::new(false));
        let threads: Vec<_> = (0..thieves)
            .map(|_| {
                let (deque, done) = (Arc::clone(&deque), Arc::clone(&done));
                std::thread::spawn(move || thief(&deque, &done))
            })
            .collect();
        let mut taken = Vec::new();
        (0..items).for_each(|item| push_and_sometimes_pop(&deque, item, &mut taken));
        while let Some(item) = deque.pop() {
            taken.push(item);
        }
        done.store(true, Ordering::Release);
        taken.extend(
            threads
                .into_iter()
                .flat_map(|t| t.join().unwrap_or_default()),
        );
        taken.sort_unstable();
        assert_eq!(taken, (0..items).collect::<Vec<_>>());
    }

    #[test]
    fn three_thieves_and_an_owner_take_every_item_exactly_once() {
        every_item_is_taken_exactly_once(if cfg!(miri) { 60 } else { 20_000 }, 3);
    }

    #[test]
    fn one_thief_and_an_owner_take_every_item_exactly_once() {
        every_item_is_taken_exactly_once(if cfg!(miri) { 60 } else { 20_000 }, 1);
    }
}
