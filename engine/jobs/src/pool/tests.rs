//! Tests of the pool: the generic contract suites at several worker counts, then what is specific
//! to a pool (refusals, shutdown, thread creation only through the port, stress, the
//! deque-full path).

use super::*;
use crate::contract;
use crate::error::JobError;
use crate::inline::InlineScheduler;
use crate::scheduler::Task;
use platform::clock::{IClock, StdClock, Ticks};
use platform::threads::{NullThreads, StdThreads, ThreadHandle};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

/// Worker counts of the contract (ADR-0004 section 6.2): the single-thread degenerate case, two,
/// odd counts (uneven halves, steal order modulo the count), the physics target, and more
/// workers than the machine of a small CI runner may have cores.
const WORKER_COUNTS: [u32; 5] = [1, 2, 3, 4, 7];

/// A short budget so the tests of the spinning path stay fast.
const SHORT: WaitPolicy = WaitPolicy::new(Duration::from_micros(50), Duration::from_micros(50));

/// A pool with the default policy. Under Miri every thread is simulated one step at a time, so
/// spinning workers would only slow the thread that has work down: there the pool parks at once
/// (the spinning path has its own test, `the_pool_passes_the_contract_with_a_spinning_policy`).
fn pool(workers: u32) -> WorkStealingPool {
    if cfg!(miri) {
        return pool_with(workers, WaitPolicy::PARK);
    }
    WorkStealingPool::new(workers, &StdThreads::new())
        .unwrap_or_else(|e| panic!("a pool of {workers} workers: {e}"))
}

fn pool_with(workers: u32, policy: WaitPolicy) -> WorkStealingPool {
    WorkStealingPool::with_policy(workers, policy, &StdThreads::new())
        .unwrap_or_else(|e| panic!("a pool of {workers} workers: {e}"))
}

/// Threads of the operating system that count how many of them are alive.
struct CountingThreads {
    inner: StdThreads,
    live: Arc<AtomicU32>,
    spawned: AtomicU32,
    /// Spawning fails from this call on (zero: never).
    fail_from: u32,
}

impl CountingThreads {
    fn new(fail_from: u32) -> Self {
        Self {
            inner: StdThreads::new(),
            live: Arc::new(AtomicU32::new(0)),
            spawned: AtomicU32::new(0),
            fail_from,
        }
    }
}

impl IThreads for CountingThreads {
    fn spawn(&self, name: &str, job: ThreadJob) -> Result<ThreadHandle, PlatformError> {
        let call = self.spawned.fetch_add(1, Ordering::Relaxed) + 1;
        if self.fail_from != 0 && call >= self.fail_from {
            return Err(PlatformError::Io);
        }
        self.live.fetch_add(1, Ordering::SeqCst);
        let live = Arc::clone(&self.live);
        self.inner.spawn(
            name,
            Box::new(move || {
                job();
                live.fetch_sub(1, Ordering::SeqCst);
            }),
        )
    }

    fn parallelism(&self) -> u32 {
        self.inner.parallelism()
    }

    fn has_threads(&self) -> bool {
        true
    }
}

/// Threads that report a given parallelism (the real ones are used to run).
struct FewCores {
    inner: StdThreads,
    cores: u32,
}

impl IThreads for FewCores {
    fn spawn(&self, name: &str, job: ThreadJob) -> Result<ThreadHandle, PlatformError> {
        self.inner.spawn(name, job)
    }

    fn parallelism(&self) -> u32 {
        self.cores
    }

    fn has_threads(&self) -> bool {
        true
    }
}

#[test]
fn the_pool_passes_the_scheduler_contract_at_every_worker_count() {
    for workers in WORKER_COUNTS {
        let pool = pool(workers);
        assert_eq!(pool.worker_count(), workers);
        contract::scheduler_contract(&pool);
    }
}

#[test]
fn the_pool_passes_the_contract_with_a_spinning_policy() {
    for workers in WORKER_COUNTS {
        contract::scheduler_contract(&pool_with(workers, SHORT));
    }
}

#[test]
fn the_pool_is_order_independent_next_to_the_test_schedulers() {
    let pools: Vec<_> = WORKER_COUNTS.iter().map(|n| pool(*n)).collect();
    let mut schedulers: Vec<&dyn IScheduler> = vec![&InlineScheduler];
    schedulers.extend(pools.iter().map(|p| p as &dyn IScheduler));
    contract::order_independence_contract(&schedulers);
}

/// D1: a per-index pure loop gives the same result at every worker count, every grain and in
/// the inline scheduler.
#[test]
fn the_result_is_identical_at_every_worker_count_and_to_the_inline_scheduler() {
    // Smaller under Miri, which simulates every step of every thread.
    let count = if cfg!(miri) { 120 } else { 5000 };
    let reference = contract::pure_loop_result(&InlineScheduler, count, 1);
    for workers in WORKER_COUNTS {
        let pool = pool(workers);
        for grain in [1, 7, 64, count, u32::MAX] {
            assert_eq!(
                contract::pure_loop_result(&pool, count, grain),
                reference,
                "{workers} workers, grain {grain}"
            );
        }
    }
}

#[test]
fn the_worker_count_is_at_least_one_and_at_most_the_maximum() {
    let max = WorkStealingPool::MAX_WORKERS;
    let cases = [
        (0, 1),
        (1, 1),
        (2, 2),
        (max, max),
        (max + 1, max),
        (u32::MAX, max),
    ];
    for (requested, expected) in cases {
        assert_eq!(WorkStealingPool::effective_workers(requested), expected);
    }
    assert_eq!(pool(0).worker_count(), 1);
}

/// Spawns the maximum number of threads, which Miri would simulate very slowly.
#[cfg(not(miri))]
#[test]
fn a_huge_request_makes_a_capped_pool() {
    let counting = CountingThreads::new(0);
    let capped =
        WorkStealingPool::new(u32::MAX, &counting).unwrap_or_else(|e| panic!("a capped pool: {e}"));
    assert_eq!(capped.worker_count(), WorkStealingPool::MAX_WORKERS);
}

#[test]
fn the_null_platform_refuses_a_pool() {
    for workers in [1, 4] {
        assert!(matches!(
            WorkStealingPool::new(workers, &NullThreads),
            Err(PlatformError::Unsupported)
        ));
    }
}

#[test]
fn a_pool_of_n_workers_starts_n_minus_one_threads_through_the_port() {
    for workers in WORKER_COUNTS {
        let counting = CountingThreads::new(0);
        let _pool = WorkStealingPool::new(workers, &counting)
            .unwrap_or_else(|e| panic!("a pool of {workers} workers: {e}"));
        assert_eq!(counting.spawned.load(Ordering::Relaxed), workers - 1);
    }
}

fn work_a_little(pool: &WorkStealingPool) {
    assert_eq!(pool.for_each(100, 3, &|_| {}), Ok(()));
    std::thread::sleep(Duration::from_millis(2));
}

#[test]
fn dropping_the_pool_joins_every_thread_even_when_they_are_parked() {
    let counting = CountingThreads::new(0);
    for _ in 0..20 {
        let pool = WorkStealingPool::new(6, &counting)
            .unwrap_or_else(|e| panic!("a pool of 6 workers: {e}"));
        // Some pools are dropped at once (workers not yet parked), some after real work.
        if counting.spawned.load(Ordering::Relaxed) % 2 == 1 {
            work_a_little(&pool);
        }
        drop(pool);
        assert_eq!(
            counting.live.load(Ordering::SeqCst),
            0,
            "every thread ended"
        );
    }
}

#[test]
fn a_failed_thread_creation_returns_the_error_and_joins_the_threads_already_started() {
    let counting = CountingThreads::new(3);
    let result = WorkStealingPool::new(6, &counting);
    assert!(matches!(result, Err(PlatformError::Io)));
    assert_eq!(counting.spawned.load(Ordering::Relaxed), 3);
    assert_eq!(counting.live.load(Ordering::SeqCst), 0, "no thread leaked");
}

#[test]
fn more_workers_than_cores_park_at_once_and_fewer_use_the_default_budget() {
    let few = FewCores {
        inner: StdThreads::new(),
        cores: 2,
    };
    let crowded = WorkStealingPool::new(3, &few).unwrap_or_else(|e| panic!("3 workers: {e}"));
    assert_eq!(crowded.wait_policy(), WaitPolicy::PARK);
    contract::scheduler_contract(&crowded);
    let fits = WorkStealingPool::new(2, &few).unwrap_or_else(|e| panic!("2 workers: {e}"));
    assert_eq!(fits.wait_policy(), WaitPolicy::DEFAULT);
}

#[test]
fn an_explicit_policy_is_kept() {
    assert_eq!(pool_with(2, SHORT).wait_policy(), SHORT);
}

fn nested_call_is_busy(pool: &WorkStealingPool) -> bool {
    pool.for_each(8, 1, &|_| {}) == Err(JobError::Busy)
}

#[test]
fn a_nested_call_is_refused_as_busy_and_the_outer_call_completes() {
    for workers in WORKER_COUNTS {
        let pool = pool(workers);
        let refused = AtomicU32::new(0);
        let ran = AtomicU32::new(0);
        let result = pool.for_each(64, 1, &|_| {
            ran.fetch_add(1, Ordering::Relaxed);
            refused.fetch_add(u32::from(nested_call_is_busy(&pool)), Ordering::Relaxed);
        });
        assert_eq!(result, Ok(()));
        assert_eq!(ran.load(Ordering::Relaxed), 64);
        assert_eq!(refused.load(Ordering::Relaxed), 64, "{workers} workers");
        assert_eq!(pool.run_scope(&[]), Ok(()), "usable after the refusals");
    }
}

/// A job body that says it started and runs until `release` is set.
fn hold_until(started: &AtomicU32, release: &AtomicU32) {
    started.store(1, Ordering::SeqCst);
    while release.load(Ordering::SeqCst) == 0 {
        std::thread::yield_now();
    }
}

#[test]
fn a_call_from_another_thread_while_one_runs_is_refused_as_busy() {
    let pool = pool(2);
    let started = AtomicU32::new(0);
    let release = AtomicU32::new(0);
    std::thread::scope(|scope| {
        let long = scope.spawn(|| pool.for_each(1, 1, &|_| hold_until(&started, &release)));
        while started.load(Ordering::SeqCst) == 0 {
            std::thread::yield_now();
        }
        assert_eq!(pool.for_each(4, 1, &|_| {}), Err(JobError::Busy));
        assert_eq!(pool.run_scope(&[&|| {}]), Err(JobError::Busy));
        release.store(1, Ordering::SeqCst);
        assert_eq!(long.join().ok(), Some(Ok(())));
    });
    assert_eq!(pool.for_each(4, 1, &|_| {}), Ok(()), "free again");
}

#[test]
fn a_refused_grain_runs_nothing_and_does_not_make_the_pool_busy() {
    let pool = pool(2);
    assert_eq!(
        pool.parallel_for(10, 0, &|_| {}),
        Err(JobError::InvalidGrain)
    );
    assert_eq!(
        pool.parallel_for(0, 0, &|_| {}),
        Err(JobError::InvalidGrain)
    );
    assert_eq!(pool.for_each(10, 1, &|_| {}), Ok(()));
}

#[test]
fn an_empty_call_returns_at_once_even_while_the_pool_is_busy() {
    let pool = pool(2);
    let inner = std::sync::atomic::AtomicBool::new(false);
    let result = pool.for_each(1, 1, &|_| {
        let nested = (pool.for_each(0, 1, &|_| {}), pool.run_scope(&[]));
        inner.store(nested == (Ok(()), Ok(())), Ordering::SeqCst);
    });
    assert_eq!(result, Ok(()));
    assert!(inner.load(Ordering::SeqCst));
}

/// One round of stress: a loop of many tiny jobs, then one of few large chunks. Adds the sum
/// of all indices to `total`.
fn stress_round(pool: &WorkStealingPool, total: &AtomicU64, small: u32, large: u32) {
    let tiny = pool.for_each(small, 1, &|i| {
        total.fetch_add(u64::from(i), Ordering::Relaxed);
    });
    assert_eq!(tiny, Ok(()));
    let chunky = pool.parallel_for(large, large / 8, &|range| {
        total.fetch_add(range.map(u64::from).sum(), Ordering::Relaxed);
    });
    assert_eq!(chunky, Ok(()));
}

/// Many tiny jobs and few huge ones.
#[test]
fn the_pool_survives_stress_with_many_small_and_few_large_jobs() {
    let (small, large, rounds) = if cfg!(miri) {
        (300, 40, 2_u32)
    } else {
        (60_000, 4_000, 30)
    };
    let per_round =
        u64::from(small) * u64::from(small - 1) / 2 + u64::from(large) * u64::from(large - 1) / 2;
    for workers in WORKER_COUNTS {
        let pool = pool_with(workers, SHORT);
        let total = AtomicU64::new(0);
        for _ in 0..rounds {
            stress_round(&pool, &total, small, large);
        }
        assert_eq!(
            total.load(Ordering::Relaxed),
            u64::from(rounds) * per_round,
            "{workers} workers"
        );
    }
}

#[test]
fn imbalanced_work_and_scopes_with_unequal_tasks_complete() {
    let pool = pool(4);
    let sink = AtomicU64::new(0);
    let heavy = || {
        let sum: u64 = (0..20_000_u64).fold(0, u64::wrapping_add);
        sink.fetch_add(sum, Ordering::Relaxed);
    };
    let light = || {
        sink.fetch_add(1, Ordering::Relaxed);
    };
    let tasks: Vec<&Task<'_>> = (0..40)
        .map(|i| {
            if i % 10 == 0 {
                &heavy as &Task<'_>
            } else {
                &light
            }
        })
        .collect();
    assert_eq!(pool.run_scope(&tasks), Ok(()));
    let heavy_sum: u64 = (0..20_000_u64).sum();
    assert_eq!(sink.load(Ordering::Relaxed), 4 * heavy_sum + 36);
}

#[test]
fn the_pool_is_created_and_dropped_over_and_over() {
    let rounds = if cfg!(miri) { 3 } else { 100 };
    for round in 0..rounds {
        let pool = pool_with(1 + round % 6, WaitPolicy::PARK);
        contract::parallel_for_contract(&pool, 33, 4);
    }
}

/// Calls the pool until `accepted` calls were accepted (a busy pool refuses and is retried).
fn call_until_accepted(pool: &WorkStealingPool, accepted: u32, ran: &AtomicU32) {
    let mut done = 0;
    while done < accepted {
        let result = pool.for_each(50, 5, &|_| {
            ran.fetch_add(1, Ordering::Relaxed);
        });
        done += u32::from(result == Ok(()));
    }
}

#[test]
fn one_pool_serves_calls_from_several_threads_one_at_a_time() {
    let pool = pool(4);
    let ran = AtomicU32::new(0);
    std::thread::scope(|scope| {
        let callers: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| call_until_accepted(&pool, 20, &ran)))
            .collect();
        assert!(callers.into_iter().all(|caller| caller.join().is_ok()));
    });
    assert_eq!(
        ran.load(Ordering::Relaxed),
        4 * 20 * 50,
        "every accepted call ran"
    );
}

#[test]
fn a_full_deque_makes_the_worker_run_the_half_itself() {
    let pool = pool(1);
    let shared = &pool.shared;
    // Fill the caller's own deque so that no half can be pushed.
    assert!((0..64_u32).all(|i| shared.deques[0].push(deque::pack(i, i))));
    let hits: Vec<AtomicU32> = (0..16).map(|_| AtomicU32::new(0)).collect();
    let body = |range: std::ops::Range<u32>| {
        for i in range {
            hits[i as usize].fetch_add(1, Ordering::Relaxed);
        }
    };
    let job = Job::for_loop(16, 1, &body).unwrap_or_else(|e| panic!("valid loop: {e}"));
    shared.published.publish(&job);
    let mut worker = Worker::new(shared, 0, WaitPolicy::PARK);
    let ran = worker.run_range(deque::pack(0, 16));
    shared.published.clear();
    assert_eq!(ran, 16, "the whole range ran in this call");
    assert!(hits.iter().all(|h| h.load(Ordering::Relaxed) == 1));
    while shared.deques[0].pop().is_some() {}
}

#[test]
fn a_worker_steals_from_every_other_deque_and_never_from_its_own() {
    let pool = pool_with(4, WaitPolicy::PARK);
    let shared = &pool.shared;
    for me in 0..4_u32 {
        let worker = Worker::new(shared, me as usize, WaitPolicy::PARK);
        assert_eq!(worker.steal(), None, "all deques are empty");
        let own = deque::pack(7, 9);
        assert!(shared.deques[me as usize].push(own));
        assert_eq!(worker.steal(), None, "never from its own deque");
        assert_eq!(shared.deques[me as usize].pop(), Some(own));
        for victim in (0..4_u32).filter(|victim| *victim != me) {
            let item = deque::pack(victim, 100);
            assert!(shared.deques[victim as usize].push(item));
            assert_eq!(worker.steal(), Some(item), "worker {me} from {victim}");
            assert_eq!(worker.steal(), None, "the victim is empty again");
        }
    }
}

#[test]
fn workers_are_woken_only_when_there_are_workers_and_more_than_one_item() {
    let one_worker = pool_with(1, WaitPolicy::PARK);
    assert_eq!(one_worker.for_each(8, 1, &|_| {}), Ok(()));
    assert_eq!(one_worker.shared.parker.wakes(), 0, "nobody to wake");
    assert_eq!(one_worker.run_scope(&[&|| {}, &|| {}]), Ok(()));
    assert_eq!(
        one_worker.shared.parker.wakes(),
        0,
        "nobody to wake for tasks"
    );
    let pool = pool_with(4, WaitPolicy::PARK);
    assert_eq!(pool.for_each(1, 1, &|_| {}), Ok(()));
    assert_eq!(pool.shared.parker.wakes(), 0, "one item cannot be shared");
    assert_eq!(pool.run_scope(&[&|| {}]), Ok(()));
    assert_eq!(pool.shared.parker.wakes(), 0, "one task cannot be shared");
    assert_eq!(pool.for_each(2, 1, &|_| {}), Ok(()));
    assert_eq!(pool.shared.parker.wakes(), 1, "two items wake the workers");
    assert_eq!(pool.run_scope(&[&|| {}, &|| {}]), Ok(()));
    assert_eq!(pool.shared.parker.wakes(), 2, "two tasks wake the workers");
}

/// The most distinct threads that ran an item of `pool` over slow items, over a few attempts.
fn threads_that_ran_items(pool: &WorkStealingPool) -> usize {
    let mut most = 0;
    for _ in 0..8 {
        let ids = std::sync::Mutex::new(std::collections::HashSet::new());
        let result = pool.for_each(64, 1, &|_| {
            std::thread::sleep(Duration::from_millis(1));
            let _ = ids
                .lock()
                .map(|mut ids| ids.insert(std::thread::current().id()));
        });
        assert_eq!(result, Ok(()));
        most = most.max(ids.lock().map_or(0, |ids| ids.len()));
        if most >= 2 {
            break;
        }
    }
    most
}

#[test]
#[cfg_attr(
    miri,
    ignore = "sleeps in real time; the contract and stress tests cover the pool under Miri"
)]
fn workers_really_run_items_with_every_policy() {
    for workers in [2, 3, 4] {
        for policy in [WaitPolicy::PARK, SHORT, WaitPolicy::DEFAULT] {
            let pool = pool_with(workers, policy);
            assert!(
                threads_that_ran_items(&pool) >= 2,
                "{workers} workers: the caller worked alone"
            );
        }
    }
}

/// Arrives and waits (at most 20 s) until `expected` threads have arrived; whether they did.
fn meet(arrived: &AtomicU32, expected: u32) -> bool {
    arrived.fetch_add(1, Ordering::SeqCst);
    let clock = StdClock::new();
    let deadline = clock.now().saturating_add(Ticks::from_millis(20_000));
    while arrived.load(Ordering::SeqCst) < expected && clock.now() < deadline {
        std::thread::yield_now();
    }
    arrived.load(Ordering::SeqCst) >= expected
}

#[test]
#[cfg_attr(
    miri,
    ignore = "waits on a real-time deadline; under Miri's virtual clock it expires before the simulated worker runs"
)]
fn as_many_tasks_as_threads_run_at_the_same_time() {
    for workers in [2_u32, 3, 4] {
        let pool = pool_with(workers, WaitPolicy::PARK);
        let arrived = AtomicU32::new(0);
        let met = AtomicU32::new(0);
        let rendezvous = || {
            met.fetch_add(u32::from(meet(&arrived, workers)), Ordering::SeqCst);
        };
        let tasks: Vec<&Task<'_>> = (0..workers).map(|_| &rendezvous as &Task<'_>).collect();
        assert_eq!(pool.run_scope(&tasks), Ok(()));
        assert_eq!(met.load(Ordering::SeqCst), workers, "{workers} workers");
    }
}
