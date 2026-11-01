//! Generic contract suites: each function takes the implementation under test, so every adapter's
//! own tests run the same checks (design: `wiki/architecture/platform-ports.md` section 4).
//! A new implementation is accepted when its suite passes unchanged.

use crate::clock::{IClock, Ticks};
use crate::error::PlatformError;
use crate::threads::IThreads;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// Clock contract: `now()` never goes backwards, `sleep(ZERO)` is harmless, and `sleep` moves time
/// forward by at least the requested span (exactly that span when `exact`, as the manual clock).
///
/// # Panics
/// When the clock breaks the contract (this is test code).
pub fn clock_contract(clock: &dyn IClock, exact: bool) {
    let mut last = clock.now();
    for _ in 0..100 {
        let now = clock.now();
        assert!(now >= last, "the clock went backwards");
        last = now;
    }
    let start = clock.now();
    clock.sleep(Ticks::ZERO);
    assert!(clock.now() >= start, "sleep(ZERO) moved time backwards");
    let wait = Ticks::from_millis(2);
    let before = clock.now();
    clock.sleep(wait);
    let waited = clock.now().saturating_sub(before);
    assert!(
        waited >= wait,
        "sleep returned early: {waited:?} < {wait:?}"
    );
    assert!(
        !exact || waited == wait,
        "sleep must advance exactly: {waited:?} != {wait:?}"
    );
}

/// Thread contract: `parallelism()` is at least 1; with `supports_threads` a spawned job runs
/// exactly once and `join` returns after it; without, `spawn` is `Unsupported` and no job runs.
///
/// # Panics
/// When the implementation breaks the contract (this is test code).
pub fn threads_contract(threads: &dyn IThreads, supports_threads: bool) {
    assert!(threads.parallelism() >= 1, "parallelism must be at least 1");
    let runs = Arc::new(AtomicU32::new(0));
    let job_runs = Arc::clone(&runs);
    let caller = std::thread::current().id();
    let other_thread = Arc::new(AtomicU32::new(0));
    let job_flag = Arc::clone(&other_thread);
    let job = Box::new(move || {
        job_runs.fetch_add(1, Ordering::SeqCst);
        job_flag.store(
            u32::from(std::thread::current().id() != caller),
            Ordering::SeqCst,
        );
    });
    let spawned = threads.spawn("contract-job", job);
    if supports_threads {
        let joined = spawned.map(crate::threads::ThreadHandle::join);
        assert_eq!(joined, Ok(Ok(())), "spawn and join must succeed");
        assert_eq!(
            runs.load(Ordering::SeqCst),
            1,
            "the job must run exactly once"
        );
        assert_eq!(
            other_thread.load(Ordering::SeqCst),
            1,
            "the job must run on its own thread"
        );
    } else {
        assert_eq!(spawned.err(), Some(PlatformError::Unsupported));
        assert_eq!(
            runs.load(Ordering::SeqCst),
            0,
            "no job may run without threads"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;
    use crate::threads::{Job, NullThreads, StdThreads, ThreadHandle};
    use std::sync::atomic::AtomicU64;

    /// Time runs backwards: every read is earlier than the one before.
    struct BackwardsClock(AtomicU64);

    impl IClock for BackwardsClock {
        fn now(&self) -> Ticks {
            let reads = self.0.fetch_add(1, Ordering::SeqCst);
            Ticks::from_nanos(1_000_u64.saturating_sub(reads))
        }

        fn sleep(&self, _duration: Ticks) {}
    }

    /// A manual clock whose sleep ignores the request.
    struct LazySleeper(ManualClock);

    impl IClock for LazySleeper {
        fn now(&self) -> Ticks {
            self.0.now()
        }

        fn sleep(&self, _duration: Ticks) {}
    }

    /// Threads that run the job inline and report success without a thread.
    struct InlineThreads;

    impl IThreads for InlineThreads {
        fn spawn(&self, _name: &str, _job: Job) -> Result<ThreadHandle, PlatformError> {
            Err(PlatformError::Io)
        }

        fn parallelism(&self) -> u32 {
            0
        }

        fn has_threads(&self) -> bool {
            false
        }
    }

    #[test]
    #[should_panic(expected = "went backwards")]
    fn the_clock_contract_rejects_a_clock_that_goes_backwards() {
        clock_contract(&BackwardsClock(AtomicU64::new(0)), false);
    }

    #[test]
    #[should_panic(expected = "returned early")]
    fn the_clock_contract_rejects_a_sleep_that_does_not_wait() {
        clock_contract(&LazySleeper(ManualClock::new()), false);
    }

    /// A manual clock whose sleep overshoots by one tick.
    struct Overshooter(ManualClock);

    impl IClock for Overshooter {
        fn now(&self) -> Ticks {
            self.0.now()
        }

        fn sleep(&self, duration: Ticks) {
            self.0.sleep(duration.saturating_add(Ticks::from_nanos(1)));
        }
    }

    fn do_nothing() {}

    /// Runs the job inline on the caller's thread.
    struct InlineRunner;

    impl IThreads for InlineRunner {
        fn spawn(&self, _name: &str, job: Job) -> Result<ThreadHandle, PlatformError> {
            job();
            let idle: Job = Box::new(do_nothing);
            StdThreads::new().spawn("inline", idle)
        }

        fn parallelism(&self) -> u32 {
            1
        }

        fn has_threads(&self) -> bool {
            true
        }
    }

    #[test]
    #[should_panic(expected = "exactly")]
    fn the_exact_clock_contract_rejects_an_overshooting_sleep() {
        clock_contract(&Overshooter(ManualClock::new()), true);
    }

    #[test]
    #[should_panic(expected = "own thread")]
    fn the_thread_contract_rejects_a_job_run_inline() {
        threads_contract(&InlineRunner, true);
    }

    #[test]
    #[should_panic(expected = "parallelism")]
    fn the_thread_contract_rejects_zero_parallelism() {
        threads_contract(&InlineThreads, false);
    }

    #[test]
    #[should_panic(expected = "spawn and join")]
    fn the_thread_contract_rejects_a_failing_spawn_when_threads_are_expected() {
        threads_contract(&NullThreads, true);
    }

    #[test]
    #[should_panic(expected = "left == right")]
    fn the_thread_contract_rejects_threads_where_none_are_expected() {
        threads_contract(&StdThreads::new(), false);
    }
}
