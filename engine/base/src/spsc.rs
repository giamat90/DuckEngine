//! A bounded lock-free queue with one producer and one consumer.
//!
//! [`channel`] builds the two ends. The [`Producer`] pushes, the [`Consumer`] pops; each end
//! can be moved to its own thread, neither end can be cloned, so there is never a second
//! producer or a second consumer. A push into a full queue gives the value back instead of
//! waiting; a pop from an empty queue returns `None`. Neither call allocates, locks or
//! blocks, so both are allowed inside an audio callback.
//!
//! The values must be `Send`; a type that is not (such as `Rc`) is refused at compile time:
//!
//! ```compile_fail
//! let _ = base::spsc::channel::<std::rc::Rc<u32>>(4);
//! ```
//!
//! Both ends are `Sync` as well as `Send`, which is harmless: a shared reference to an end can
//! only ask `capacity` and `is_full` or `is_empty`, never move a value.
//!
//! The ring has a power-of-two number of slots. The producer owns the `tail` counter and the
//! consumer owns the `head` counter; each reads the other's counter with an acquire load and
//! publishes its own with a release store, which is what makes the slot contents visible to the
//! other thread. The counters run freely and wrap; their difference is the length.
#![expect(
    unsafe_code,
    reason = "slots are shared between two threads through UnsafeCell; every unsafe block has a SAFETY comment"
)]

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The largest capacity [`channel`] accepts (2^30 slots).
pub const MAX_CAPACITY: u32 = 1 << 30;

struct Shared<T> {
    slots: Box<[UnsafeCell<MaybeUninit<T>>]>,
    /// Next slot to read; written by the consumer only.
    head: AtomicUsize,
    /// Next slot to write; written by the producer only.
    tail: AtomicUsize,
}

// SAFETY: a slot is touched by one thread at a time. The producer writes a slot only while the
// consumer's `head` shows it is free, the consumer reads it only after the producer's `tail`
// (a release store) showed it is full, and `T: Send` allows the value to change threads.
unsafe impl<T: Send> Sync for Shared<T> {}

impl<T> Shared<T> {
    fn mask(&self) -> usize {
        self.slots.len() - 1
    }

    fn drop_slot(&mut self, index: usize) {
        if let Some(slot) = self.slots.get_mut(index) {
            // SAFETY: only called for slots from `head` up to `tail`, which were written and
            // not yet read.
            unsafe { slot.get_mut().assume_init_drop() };
        }
    }
}

impl<T> Drop for Shared<T> {
    fn drop(&mut self) {
        // Both ends are gone, so no other thread can touch the slots; the counters are exact.
        let mask = self.mask();
        let tail = *self.tail.get_mut();
        let mut head = *self.head.get_mut();
        while head != tail {
            self.drop_slot(head & mask);
            head = head.wrapping_add(1);
        }
    }
}

/// The sending end; see the module documentation.
pub struct Producer<T> {
    shared: Arc<Shared<T>>,
}

/// The receiving end; see the module documentation.
pub struct Consumer<T> {
    shared: Arc<Shared<T>>,
}

/// Builds a queue with room for at least `capacity` values (rounded up to a power of two).
/// Returns `None` for 0 or more than [`MAX_CAPACITY`]. All allocation happens here.
#[must_use]
pub fn channel<T: Send>(capacity: u32) -> Option<(Producer<T>, Consumer<T>)> {
    if capacity == 0 || capacity > MAX_CAPACITY {
        return None;
    }
    let slots = usize::try_from(capacity.next_power_of_two()).ok()?;
    let shared = Arc::new(Shared {
        slots: (0..slots)
            .map(|_| UnsafeCell::new(MaybeUninit::uninit()))
            .collect(),
        head: AtomicUsize::new(0),
        tail: AtomicUsize::new(0),
    });
    Some((
        Producer {
            shared: Arc::clone(&shared),
        },
        Consumer { shared },
    ))
}

impl<T> Producer<T> {
    /// The number of values the queue holds at most.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.shared.slots.len()
    }

    /// True when a push would be refused right now. The consumer can only make room, so a
    /// queue that is not full stays not full until this end pushes.
    #[must_use]
    pub fn is_full(&self) -> bool {
        let tail = self.shared.tail.load(Ordering::Relaxed);
        let head = self.shared.head.load(Ordering::Acquire);
        tail.wrapping_sub(head) == self.capacity()
    }

    /// Appends `value`.
    ///
    /// # Errors
    /// Gives `value` back when the queue is full.
    pub fn push(&mut self, value: T) -> Result<(), T> {
        if self.is_full() {
            return Err(value);
        }
        let tail = self.shared.tail.load(Ordering::Relaxed);
        let Some(slot) = self.shared.slots.get(tail & self.shared.mask()) else {
            return Err(value);
        };
        // SAFETY: the queue is not full, so the consumer is done with this slot and, being the
        // only producer (`&mut self`), nobody else writes it.
        unsafe { (*slot.get()).write(value) };
        self.shared
            .tail
            .store(tail.wrapping_add(1), Ordering::Release);
        Ok(())
    }
}

impl<T> Consumer<T> {
    /// True when nothing can be popped right now.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let head = self.shared.head.load(Ordering::Relaxed);
        head == self.shared.tail.load(Ordering::Acquire)
    }

    /// Removes and returns the oldest value, or `None` when the queue is empty.
    pub fn pop(&mut self) -> Option<T> {
        if self.is_empty() {
            return None;
        }
        let head = self.shared.head.load(Ordering::Relaxed);
        let slot = self.shared.slots.get(head & self.shared.mask())?;
        // SAFETY: the queue is not empty, so the producer wrote this slot and published it with
        // a release store that the acquire load above saw; being the only consumer
        // (`&mut self`), nobody else reads it, and the producer will not reuse it before the
        // `head` store below.
        let value = unsafe { (*slot.get()).assume_init_read() };
        self.shared
            .head
            .store(head.wrapping_add(1), Ordering::Release);
        Some(value)
    }
}
