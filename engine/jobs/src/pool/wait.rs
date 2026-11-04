//! Waiting: the bounded wait policy of ADR-0004 (spin, then yield, then park, within a time
//! budget), the latch the caller joins on and the parker that wakes idle workers.

use platform::clock::{IClock, StdClock, Ticks};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// Share of a bounded budget that is spent spinning (the rest yields): one in this many.
const SPIN_SHARE: u64 = 4;

/// The default time budget before a thread parks, in microseconds (card 4-7, ADR-0004 point 3).
///
/// Chosen from `cargo run --release -p tools_scheduler_spike --example wait_budget` (Windows 11 only, 4 workers, 3
/// sessions, noisy; the numbers are in `reviews/4-7.md`). The clear gain is on back-to-back
/// phases: 100 dependent phases of 512 indices cost 5.9 us per phase at 200 us against 17.5 us
/// when parking at once (3.0 times more); on Linux (WSL, 2 sessions) 2.4 against 18.3 us. For the
/// join of 24 trivial items with a gap before the next call, the Windows medians are within noise
/// of parking at once up to a gap of 200 us (3.7 to 4.9 against 4.4 to 4.8 us); a gap just above
/// the budget is the worst case, because the worker has just parked (a gap of 200 us costs 32 to
/// 37 us at a budget of 50 to 100 us). 1000 us multiplies the spinning time of every idle worker by five
/// without a gain on the phases. The cost is at most 200 us of CPU per worker per idle episode,
/// and none when the pool has more workers than free cores (it parks at once then).
const DEFAULT_BUDGET_MICROS: u64 = 200;

/// How long a thread that has nothing to do keeps looking before it parks. The form is fixed by
/// ADR-0004 (bounded, never an unbounded spin); the numbers are measured, see [`WaitPolicy::DEFAULT`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaitPolicy {
    spin: Duration,
    yielding: Duration,
}

impl WaitPolicy {
    /// Never spin or yield: park at once. The policy for a pool with more workers than free cores,
    /// where a spinning worker would take the core of one that has work (ADR-0004).
    pub const PARK: Self = Self::new(Duration::ZERO, Duration::ZERO);

    /// The default: a budget of 200 microseconds (see `DEFAULT_BUDGET_MICROS`).
    pub const DEFAULT: Self = Self::bounded_micros(DEFAULT_BUDGET_MICROS);

    /// A policy with a time budget of `micros` microseconds before parking: the first
    /// `1 / SPIN_SHARE` of it spins, the rest yields the time slice.
    #[must_use]
    pub const fn bounded_micros(micros: u64) -> Self {
        let spin = micros / SPIN_SHARE;
        Self::new(
            Duration::from_micros(spin),
            Duration::from_micros(micros - spin),
        )
    }

    /// A policy that spins for `spin`, then yields the time slice for `yielding`, then parks.
    #[must_use]
    pub const fn new(spin: Duration, yielding: Duration) -> Self {
        Self { spin, yielding }
    }

    /// The whole time budget before parking.
    #[must_use]
    pub fn budget(self) -> Duration {
        self.spin + self.yielding
    }
}

/// What a waiting thread does next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Busy-wait a moment (`spin_loop`).
    Spin,
    /// Give the time slice away (`yield_now`).
    Yield,
    /// The budget is spent: block until woken.
    Park,
}

/// One episode of waiting under a [`WaitPolicy`]: the clock starts at the first call of
/// [`phase`](Self::phase).
pub struct Backoff {
    policy: WaitPolicy,
    clock: StdClock,
    started: Option<Ticks>,
}

impl Backoff {
    /// A fresh episode.
    #[must_use]
    pub fn new(policy: WaitPolicy, clock: StdClock) -> Self {
        Self {
            policy,
            clock,
            started: None,
        }
    }

    /// Starts a new episode (the thread found work).
    pub fn reset(&mut self) {
        self.started = None;
    }

    /// The phase the episode is in now.
    pub fn phase(&mut self) -> Phase {
        let budget = self.policy.budget();
        if budget.is_zero() {
            return Phase::Park;
        }
        let now = self.clock.now();
        let started = *self.started.get_or_insert(now);
        let elapsed = Duration::from_nanos(now.saturating_sub(started).as_nanos());
        if elapsed < self.policy.spin {
            Phase::Spin
        } else if elapsed < budget {
            Phase::Yield
        } else {
            Phase::Park
        }
    }

    /// Waits one step in the current phase. False when the budget is spent and the thread
    /// should park.
    pub fn pause(&mut self) -> bool {
        match self.phase() {
            Phase::Spin => {
                std::hint::spin_loop();
                true
            }
            Phase::Yield => {
                std::thread::yield_now();
                true
            }
            Phase::Park => false,
        }
    }
}

/// Locks, ignoring poisoning (the build aborts on panic, so a poisoned lock cannot be observed).
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Wakes idle workers: each call to [`wake_all`](Self::wake_all) bumps a generation, workers
/// wait for the generation to change.
pub struct Parker {
    generation: AtomicU64,
    shutdown: AtomicBool,
    gate: Mutex<()>,
    wake: Condvar,
}

impl Parker {
    /// A parker with nobody woken yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            generation: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
            gate: Mutex::new(()),
            wake: Condvar::new(),
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

    /// How many times workers were woken (for tests).
    #[cfg(test)]
    pub fn wakes(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Ends the workers' loops.
    pub fn close(&self) {
        self.shutdown.store(true, Ordering::Release);
        drop(lock(&self.gate));
        self.wake.notify_all();
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

    /// Blocks until the generation differs from `*seen` (then stores it) or the parker is
    /// closed. Returns false when closed.
    pub fn wait(&self, seen: &mut u64) -> bool {
        let mut gate = lock(&self.gate);
        let mut verdict = self.verdict(seen);
        while verdict.is_none() {
            gate = self.wake.wait(gate).unwrap_or_else(PoisonError::into_inner);
            verdict = self.verdict(seen);
        }
        verdict == Some(true)
    }
}

impl Default for Parker {
    fn default() -> Self {
        Self::new()
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

    /// Waits until the count is zero: looks under `policy`, then parks.
    pub fn wait(&self, policy: WaitPolicy, clock: StdClock) {
        let mut backoff = Backoff::new(policy, clock);
        while !self.reached_zero() && backoff.pause() {}
        if !self.reached_zero() {
            self.park();
        }
    }

    fn park(&self) {
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

    const MILLI: Duration = Duration::from_millis(1);

    /// A policy with a budget long enough to observe, short enough to keep the suite fast.
    fn policy(spin_ms: u32, yield_ms: u32) -> WaitPolicy {
        WaitPolicy::new(MILLI * spin_ms, MILLI * yield_ms)
    }

    #[test]
    fn the_budget_is_spin_plus_yield() {
        assert_eq!(policy(3, 4).budget(), MILLI * 7);
        assert_eq!(WaitPolicy::PARK.budget(), Duration::ZERO);
    }

    #[test]
    fn a_bounded_budget_spins_a_quarter_and_yields_the_rest() {
        let bounded = WaitPolicy::bounded_micros(100);
        assert_eq!(bounded.spin, Duration::from_micros(25));
        assert_eq!(bounded.yielding, Duration::from_micros(75));
        assert_eq!(bounded.budget(), Duration::from_micros(100));
        assert_eq!(WaitPolicy::bounded_micros(0), WaitPolicy::PARK);
        assert_eq!(
            WaitPolicy::bounded_micros(3).budget(),
            Duration::from_micros(3)
        );
    }

    #[test]
    fn the_default_policy_is_the_measured_budget() {
        assert_eq!(WaitPolicy::DEFAULT, WaitPolicy::bounded_micros(200));
        assert_eq!(WaitPolicy::DEFAULT.budget(), Duration::from_micros(200));
    }

    #[test]
    fn a_zero_budget_parks_at_once() {
        let mut backoff = Backoff::new(WaitPolicy::PARK, StdClock::new());
        assert_eq!(backoff.phase(), Phase::Park);
        assert!(!backoff.pause());
    }

    /// The phases an episode goes through, polling only the clock (not `pause`): a yield can take
    /// a whole time slice on a loaded machine and would make the observed phases depend on the
    /// scheduler.
    fn phases_of(policy: WaitPolicy) -> Vec<Phase> {
        let mut backoff = Backoff::new(policy, StdClock::new());
        let mut seen = Vec::new();
        while seen.last() != Some(&Phase::Park) {
            seen.push(backoff.phase());
            seen.dedup();
        }
        seen
    }

    #[test]
    fn an_episode_spins_then_yields_then_parks() {
        let clock = StdClock::new();
        let begun = clock.now();
        assert_eq!(
            phases_of(policy(20, 20)),
            [Phase::Spin, Phase::Yield, Phase::Park]
        );
        assert!(
            elapsed_since(&clock, begun) >= MILLI * 40,
            "the whole budget was used"
        );
    }

    #[test]
    fn a_spin_only_policy_never_yields_and_a_yield_only_policy_never_spins() {
        assert_eq!(phases_of(policy(5, 0)), [Phase::Spin, Phase::Park]);
        assert_eq!(phases_of(policy(0, 5)), [Phase::Yield, Phase::Park]);
    }

    #[test]
    fn pausing_in_each_phase_reports_whether_to_keep_waiting() {
        let mut backoff = Backoff::new(policy(1_000, 1_000), StdClock::new());
        assert!(backoff.pause(), "spinning");
        let mut yielding = Backoff::new(policy(0, 1_000), StdClock::new());
        assert!(yielding.pause(), "yielding");
    }

    #[test]
    fn a_reset_starts_a_new_episode() {
        let mut backoff = Backoff::new(policy(2, 2), StdClock::new());
        while backoff.phase() != Phase::Park {}
        assert_eq!(backoff.phase(), Phase::Park);
        backoff.reset();
        assert_eq!(backoff.phase(), Phase::Spin);
    }

    #[test]
    fn a_latch_at_zero_does_not_wait() {
        let latch = Latch::new();
        latch.wait(WaitPolicy::PARK, StdClock::new());
        latch.wait(policy(1, 1), StdClock::new());
    }

    fn count_down(latch: &Latch) {
        std::thread::sleep(MILLI * 5);
        latch.done(1);
        latch.done(2);
    }

    #[test]
    fn a_latch_releases_the_waiter_when_the_last_item_is_done() {
        for wait_policy in [WaitPolicy::PARK, policy(1, 1), policy(50, 50)] {
            let latch = Arc::new(Latch::new());
            latch.arm(3);
            let other = Arc::clone(&latch);
            let helper = std::thread::spawn(move || count_down(&other));
            latch.wait(wait_policy, StdClock::new());
            assert_eq!(latch.remaining.load(Ordering::Acquire), 0);
            assert!(helper.join().is_ok());
        }
    }

    #[test]
    fn a_latch_that_is_not_at_zero_keeps_the_waiter() {
        let latch = Arc::new(Latch::new());
        latch.arm(2);
        let other = Arc::clone(&latch);
        let helper = std::thread::spawn(move || {
            std::thread::sleep(MILLI * 20);
            other.done(1);
            std::thread::sleep(MILLI * 20);
            other.done(1);
        });
        let clock = StdClock::new();
        let begun = clock.now();
        latch.wait(WaitPolicy::PARK, StdClock::new());
        assert!(
            elapsed_since(&clock, begun) >= MILLI * 30,
            "waited for both items"
        );
        assert!(helper.join().is_ok());
    }

    fn elapsed_since(clock: &StdClock, begun: Ticks) -> Duration {
        Duration::from_nanos(clock.now().saturating_sub(begun).as_nanos())
    }

    fn wait_twice(parker: &Parker) -> (bool, bool) {
        let mut seen = 0;
        (parker.wait(&mut seen), parker.wait(&mut seen))
    }

    #[test]
    fn a_parker_wakes_a_waiter_and_then_closes() {
        let parker = Arc::new(Parker::new());
        let other = Arc::clone(&parker);
        let helper = std::thread::spawn(move || wait_twice(&other));
        parker.wake_all();
        // Long enough for a loaded machine to start the helper before the close.
        std::thread::sleep(MILLI * 200);
        parker.close();
        assert_eq!(helper.join().ok(), Some((true, false)));
    }

    #[test]
    fn a_latch_that_is_already_zero_returns_without_using_its_budget() {
        let latch = Latch::new();
        let clock = StdClock::new();
        let begun = clock.now();
        latch.wait(policy(500, 500), StdClock::new());
        assert!(
            elapsed_since(&clock, begun) < MILLI * 400,
            "the budget was spent on a zero latch"
        );
    }

    #[test]
    fn a_wake_before_the_wait_is_not_lost() {
        let parker = Parker::new();
        parker.wake_all();
        let mut seen = 0;
        assert!(parker.wait(&mut seen));
        assert_eq!(seen, 1);
    }

    #[test]
    fn a_closed_parker_never_blocks() {
        let parker = Parker::default();
        parker.close();
        let mut seen = 0;
        assert!(!parker.wait(&mut seen));
        assert!(!parker.wait(&mut seen));
    }
}
