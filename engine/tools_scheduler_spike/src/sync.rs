//! The two waiting primitives every candidate uses: a parker that wakes idle workers for the
//! next call and a latch the caller waits on until all items are done.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

/// How long the caller spins on the latch before it parks (iterations of `spin_loop`). The
/// calling thread is hot, so a short spin is the same for every candidate.
const JOIN_SPIN: u32 = 2000;

/// Locks, ignoring poisoning (the build aborts on panic, so a poisoned lock cannot be observed).
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Wakes idle workers: each call to [`wake_all`](Self::wake_all) bumps a generation, workers
/// wait for the generation to change. With `idle_spin` above zero a worker spins that many
/// iterations before it parks on the condvar.
pub struct Parker {
    generation: AtomicU64,
    shutdown: AtomicBool,
    gate: Mutex<()>,
    wake: Condvar,
    idle_spin: u32,
}

impl Parker {
    /// A parker whose workers spin `idle_spin` iterations before they park.
    #[must_use]
    pub fn new(idle_spin: u32) -> Self {
        Self {
            generation: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
            gate: Mutex::new(()),
            wake: Condvar::new(),
            idle_spin,
        }
    }

    /// Wakes every waiting worker for a new call.
    pub fn wake_all(&self) {
        self.generation.fetch_add(1, Ordering::Release);
        // Taking the gate orders this notify after any waiter that checked the generation and is
        // about to wait: it either saw the new generation or is already waiting.
        drop(lock(&self.gate));
        self.wake.notify_all();
    }

    /// Ends the workers' loops.
    pub fn close(&self) {
        self.shutdown.store(true, Ordering::Release);
        drop(lock(&self.gate));
        self.wake.notify_all();
    }

    /// True when the generation moved past `seen` or the parker is closed.
    fn changed(&self, seen: u64) -> bool {
        self.generation.load(Ordering::Acquire) != seen || self.shutdown.load(Ordering::Acquire)
    }

    fn spin_once(&self, seen: u64) -> bool {
        let changed = self.changed(seen);
        if !changed {
            std::hint::spin_loop();
        }
        changed
    }

    /// `Some(answer)` when [`wait`](Self::wait) can return, `None` to keep waiting.
    fn verdict(&self, seen: &mut u64) -> Option<bool> {
        if self.shutdown.load(Ordering::Acquire) {
            return Some(false);
        }
        let current = self.generation.load(Ordering::Acquire);
        let moved = current != *seen;
        *seen = current;
        moved.then_some(true)
    }

    /// Waits until the generation differs from `*seen` (then stores it) or the parker is closed.
    /// Returns false when closed.
    pub fn wait(&self, seen: &mut u64) -> bool {
        let start = *seen;
        let _ = (0..self.idle_spin).any(|_| self.spin_once(start));
        let mut gate = lock(&self.gate);
        let mut verdict = self.verdict(seen);
        while verdict.is_none() {
            gate = self.wake.wait(gate).unwrap_or_else(PoisonError::into_inner);
            verdict = self.verdict(seen);
        }
        verdict == Some(true)
    }
}

/// Counts the items still to do; the caller waits for zero. Counting down uses release/acquire,
/// so everything the jobs wrote is visible to the caller after [`wait`](Self::wait) (the
/// happens-before edge of the join).
pub struct Latch {
    remaining: AtomicU32,
    gate: Mutex<()>,
    zero: Condvar,
}

impl Latch {
    /// A latch at zero.
    #[must_use]
    pub fn new() -> Self {
        Self {
            remaining: AtomicU32::new(0),
            gate: Mutex::new(()),
            zero: Condvar::new(),
        }
    }

    /// Sets the count. Call before any item can be done.
    pub fn arm(&self, items: u32) {
        self.remaining.store(items, Ordering::Relaxed);
    }

    /// Counts `items` as done.
    pub fn done(&self, items: u32) {
        if self.remaining.fetch_sub(items, Ordering::AcqRel) == items {
            drop(lock(&self.gate));
            self.zero.notify_all();
        }
    }

    fn reached_zero(&self) -> bool {
        self.remaining.load(Ordering::Acquire) == 0
    }

    fn spin_once(&self) -> bool {
        let zero = self.reached_zero();
        if !zero {
            std::hint::spin_loop();
        }
        zero
    }

    /// Waits until the count is zero: a short spin, then parks.
    pub fn wait(&self) {
        if (0..JOIN_SPIN).any(|_| self.spin_once()) {
            return;
        }
        let mut gate = lock(&self.gate);
        while !self.reached_zero() {
            gate = self.zero.wait(gate).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

impl Default for Latch {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn count_down(latch: &Latch) {
        latch.done(1);
        latch.done(2);
    }

    fn wait_twice(parker: &Parker) -> (bool, bool) {
        let mut seen = 0;
        (parker.wait(&mut seen), parker.wait(&mut seen))
    }

    #[test]
    fn a_latch_releases_the_waiter_when_the_last_item_is_done() {
        let latch = Arc::new(Latch::new());
        latch.arm(3);
        let other = Arc::clone(&latch);
        let helper = std::thread::spawn(move || count_down(&other));
        latch.wait();
        assert!(helper.join().is_ok());
    }

    #[test]
    fn a_parker_wakes_a_waiter_and_then_closes() {
        let parker = Arc::new(Parker::new(0));
        let other = Arc::clone(&parker);
        let helper = std::thread::spawn(move || wait_twice(&other));
        parker.wake_all();
        std::thread::sleep(std::time::Duration::from_millis(20));
        parker.close();
        assert_eq!(helper.join().ok(), Some((true, false)));
    }

    #[test]
    fn a_spinning_parker_sees_the_new_generation() {
        let parker = Parker::new(10);
        parker.wake_all();
        let mut seen = 0;
        assert!(parker.wait(&mut seen));
        assert_eq!(seen, 1);
    }
}
