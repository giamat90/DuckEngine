//! The clock port: integer nanosecond ticks on a monotonic clock.
//!
//! [`StdClock`] reads `std::time::Instant`; [`ManualClock`] is the Null implementation, whose time
//! moves only when a test or a headless app calls [`ManualClock::advance`]. No wall-clock time in
//! v0. Integers, not float seconds, so an accumulator built on ticks never drifts.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// A point or a span of monotonic time, in nanoseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ticks(u64);

impl Ticks {
    /// No time.
    pub const ZERO: Self = Self(0);

    /// `nanos` nanoseconds.
    #[must_use]
    pub const fn from_nanos(nanos: u64) -> Self {
        Self(nanos)
    }

    /// `micros` microseconds, saturating at the largest value.
    #[must_use]
    pub const fn from_micros(micros: u64) -> Self {
        Self(micros.saturating_mul(1_000))
    }

    /// `millis` milliseconds, saturating at the largest value.
    #[must_use]
    pub const fn from_millis(millis: u64) -> Self {
        Self(millis.saturating_mul(1_000_000))
    }

    /// The count of nanoseconds.
    #[must_use]
    pub const fn as_nanos(self) -> u64 {
        self.0
    }

    /// Sum, saturating at the largest value.
    #[must_use]
    pub const fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    /// Difference, zero when `other` is later.
    #[must_use]
    pub const fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }
}

/// A monotonic clock; see the module documentation.
pub trait IClock: Send + Sync {
    /// The time since the clock's origin. Never goes backwards.
    fn now(&self) -> Ticks;

    /// Waits at least `duration` (the `std` clock) or moves the clock by exactly `duration` (the
    /// manual clock). For the application's frame pacing only.
    fn sleep(&self, duration: Ticks);
}

/// The clock of the operating system, with its origin at construction.
#[derive(Clone, Copy, Debug)]
pub struct StdClock {
    origin: Instant,
}

impl StdClock {
    /// A clock whose `now()` starts near zero.
    #[must_use]
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for StdClock {
    fn default() -> Self {
        Self::new()
    }
}

impl IClock for StdClock {
    fn now(&self) -> Ticks {
        Ticks(u64::try_from(self.origin.elapsed().as_nanos()).unwrap_or(u64::MAX))
    }

    fn sleep(&self, duration: Ticks) {
        let target = self.now().saturating_add(duration);
        // `thread::sleep` may return early on some systems: wait out the rest.
        while self.now() < target {
            let left = target.saturating_sub(self.now());
            std::thread::sleep(Duration::from_nanos(left.as_nanos()));
        }
    }
}

/// The Null clock: time moves only by [`advance`](Self::advance) or [`IClock::sleep`].
#[derive(Debug, Default)]
pub struct ManualClock {
    nanos: AtomicU64,
}

impl ManualClock {
    /// A clock at time zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Moves the clock forward by exactly `by` (saturating at the largest value).
    pub fn advance(&self, by: Ticks) {
        // The closure always returns `Some`, so the update cannot fail.
        let _ = self
            .nanos
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |t| {
                Some(t.saturating_add(by.0))
            });
    }
}

impl IClock for ManualClock {
    fn now(&self) -> Ticks {
        Ticks(self.nanos.load(Ordering::SeqCst))
    }

    fn sleep(&self, duration: Ticks) {
        self.advance(duration);
    }
}

/// A shared clock, so the test or headless app keeps a handle to a manual clock after it is boxed
/// into a [`Platform`](crate::platform::Platform).
impl<C: IClock + ?Sized> IClock for Arc<C> {
    fn now(&self) -> Ticks {
        (**self).now()
    }

    fn sleep(&self, duration: Ticks) {
        (**self).sleep(duration);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract;

    #[test]
    fn ticks_convert_and_saturate() {
        assert_eq!(Ticks::from_millis(3).as_nanos(), 3_000_000);
        assert_eq!(Ticks::from_micros(5).as_nanos(), 5_000);
        assert_eq!(Ticks::from_millis(u64::MAX).as_nanos(), u64::MAX);
        assert_eq!(Ticks::from_micros(u64::MAX).as_nanos(), u64::MAX);
        assert_eq!(Ticks::ZERO, Ticks::default());
    }

    #[test]
    fn ticks_add_and_subtract_without_wrapping() {
        let (a, b) = (Ticks::from_nanos(7), Ticks::from_nanos(3));
        assert_eq!(a.saturating_add(b).as_nanos(), 10);
        assert_eq!(a.saturating_sub(b).as_nanos(), 4);
        assert_eq!(b.saturating_sub(a), Ticks::ZERO);
        assert_eq!(
            Ticks::from_nanos(u64::MAX).saturating_add(a).as_nanos(),
            u64::MAX
        );
    }

    #[test]
    fn the_manual_clock_starts_at_zero_and_moves_by_exactly_the_request() {
        let clock = ManualClock::new();
        assert_eq!(clock.now(), Ticks::ZERO);
        clock.advance(Ticks::from_millis(3));
        clock.advance(Ticks::from_nanos(1));
        assert_eq!(clock.now().as_nanos(), 3_000_001);
    }

    #[test]
    fn the_manual_clock_does_not_move_on_its_own() {
        let clock = ManualClock::new();
        assert_eq!(clock.now(), clock.now());
    }

    #[test]
    fn manual_sleep_advances_by_exactly_the_request() {
        let clock = ManualClock::new();
        clock.sleep(Ticks::from_micros(250));
        assert_eq!(clock.now(), Ticks::from_micros(250));
    }

    #[test]
    fn advance_saturates() {
        let clock = ManualClock::new();
        clock.advance(Ticks::from_nanos(u64::MAX));
        clock.advance(Ticks::from_nanos(5));
        assert_eq!(clock.now().as_nanos(), u64::MAX);
    }

    #[test]
    fn a_shared_clock_is_seen_through_the_arc() {
        let clock = Arc::new(ManualClock::new());
        let boxed: Box<dyn IClock> = Box::new(Arc::clone(&clock));
        clock.advance(Ticks::from_nanos(9));
        assert_eq!(boxed.now().as_nanos(), 9);
        boxed.sleep(Ticks::from_nanos(1));
        assert_eq!(clock.now().as_nanos(), 10);
    }

    #[test]
    fn the_std_clock_passes_the_contract() {
        contract::clock_contract(&StdClock::new(), false);
    }

    #[test]
    fn the_manual_clock_passes_the_contract() {
        contract::clock_contract(&ManualClock::new(), true);
    }
}
