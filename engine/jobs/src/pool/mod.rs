//! The work-stealing pool (ADR-0004, card 4-7): the first real [`IScheduler`].
//!
//! Every worker owns a fixed-capacity lock-free Chase-Lev deque of item ranges. A call starts
//! with the range of all its items on the caller's deque; whoever holds a range of more than one
//! item pushes its upper half on its own deque and keeps splitting the lower half until one item
//! is left, which it runs (binary splitting). Idle workers steal the oldest, largest range of
//! another worker. The caller is worker 0: it runs its share, then joins on a latch.
//!
//! * **Threads** are created only through the [`IThreads`] port, at construction, and joined when
//!   the pool is dropped. The caller plus `workers - 1` spawned threads run jobs.
//! * **Waiting** follows the bounded [`WaitPolicy`] of ADR-0004: after a call a worker keeps
//!   scanning (spin, then yield) for a time budget before it parks.
//! * **No allocation** after construction: deques, latch and parker are built once.
//! * **Panics.** A job must not panic. The workspace builds with `panic = abort`; `cargo test`
//!   unwinds, so the work loops hold an abort guard (see the `erased` module): a panicking job
//!   ends the process rather than leaving workers with a dangling job or the caller in the latch.
//! * **One call at a time.** A second call (nested, or from another thread) while one runs is
//!   refused with [`JobError::Busy`].
//! * `unsafe` is confined to the `erased` module (lifetime erasure of the borrowed job).

mod deque;
mod erased;
mod job;
#[cfg(test)]
mod tests;
mod wait;

pub use wait::WaitPolicy;

use crate::error::JobError;
use crate::scheduler::{IScheduler, RangeBody, Task};
use deque::{Deque, pack, unpack};
use erased::{AbortOnUnwind, Published};
use job::Job;
use platform::clock::StdClock;
use platform::error::PlatformError;
use platform::threads::{IThreads, Job as ThreadJob, ThreadHandle};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use wait::{Backoff, Latch, Parker};

/// State shared by the caller and the worker threads.
struct Shared {
    deques: Vec<Deque>,
    published: Published,
    latch: Latch,
    parker: Parker,
    busy: AtomicBool,
    policy: WaitPolicy,
    clock: StdClock,
}

/// One thread's view of a call: its deque number, the items it ran and has not yet counted on
/// the latch, and its idle episode.
struct Worker<'a> {
    shared: &'a Shared,
    me: usize,
    executed: u32,
    backoff: Backoff,
}

impl<'a> Worker<'a> {
    fn new(shared: &'a Shared, me: usize, policy: WaitPolicy) -> Self {
        Self {
            shared,
            me,
            executed: 0,
            backoff: Backoff::new(policy, shared.clock),
        }
    }

    /// Takes the oldest range of another worker.
    fn steal(&self) -> Option<u64> {
        let count = self.shared.deques.len();
        (1..count).find_map(|offset| self.shared.deques[(self.me + offset) % count].steal())
    }

    /// Runs the items `lo..hi` one after another; returns how many.
    fn run_all(&self, lo: u32, hi: u32) -> u32 {
        for index in lo..hi {
            self.shared.published.run_item(index);
        }
        hi.saturating_sub(lo)
    }

    /// Pushes the range `lo..hi` on this worker's deque for others to steal; returns how many
    /// items it ran instead (all of them when the deque was full, otherwise none).
    fn defer(&self, lo: u32, hi: u32) -> u32 {
        if self.shared.deques[self.me].push(pack(lo, hi)) {
            0
        } else {
            self.run_all(lo, hi)
        }
    }

    /// Splits the range down to one item and runs that; returns how many items this ran (one,
    /// or more when a deque was full and a half had to run here).
    fn run_range(&mut self, item: u64) -> u32 {
        let (lo, mut hi) = unpack(item);
        let mut ran = 0;
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            ran += self.defer(mid, hi);
            hi = mid;
        }
        ran + self.run_all(lo, hi)
    }

    /// One step inside a call: take a range (own deque first, then steal) and run it, or count
    /// what was run on the latch and wait a moment. False when the budget is spent.
    fn step(&mut self, first: &mut Option<u64>) -> bool {
        let item = first
            .take()
            .or_else(|| self.shared.deques[self.me].pop())
            .or_else(|| self.steal());
        if let Some(item) = item {
            self.backoff.reset();
            self.executed += self.run_range(item);
            return true;
        }
        if self.executed > 0 {
            self.shared.latch.done(self.executed);
            self.executed = 0;
        }
        self.backoff.pause()
    }

    /// The loop of a thread inside a call, starting from `first`, until the budget is spent.
    fn work(&mut self, mut first: Option<u64>) {
        let _guard = AbortOnUnwind;
        while self.step(&mut first) {}
    }
}

impl Shared {
    fn run(&self, job: &Job<'_>, wake: bool) -> Result<(), JobError> {
        let total = job.items();
        if total == 0 {
            return Ok(());
        }
        if self.busy.swap(true, Ordering::Acquire) {
            return Err(JobError::Busy);
        }
        self.published.publish(job);
        self.latch.arm(total);
        // The whole range goes into the caller's deque before the wake, so a worker that wakes
        // early finds something to steal instead of parking again (an empty deque cannot be full).
        let range = pack(0, total);
        let first = (!self.deques[0].push(range)).then_some(range);
        if wake && total > 1 {
            self.parker.wake_all();
        }
        Worker::new(self, 0, WaitPolicy::PARK).work(first);
        self.latch.wait(self.policy, self.clock);
        self.published.clear();
        self.busy.store(false, Ordering::Release);
        Ok(())
    }
}

fn worker_main(shared: &Shared, me: usize) {
    let mut seen = 0;
    while shared.parker.wait(&mut seen) {
        Worker::new(shared, me, shared.policy).work(None);
    }
}

/// A pool of worker threads with per-worker lock-free deques and work stealing.
pub struct WorkStealingPool {
    shared: Arc<Shared>,
    workers: u32,
    threads: Vec<ThreadHandle>,
}

impl WorkStealingPool {
    /// The most workers a pool has (a larger request is capped).
    pub const MAX_WORKERS: u32 = 256;

    /// A pool of `workers` threads at the same time, the calling thread included (at least 1, at
    /// most [`MAX_WORKERS`](Self::MAX_WORKERS)). With more workers than the platform has free
    /// cores the pool parks at once instead of spinning (ADR-0004); otherwise
    /// [`WaitPolicy::DEFAULT`] applies.
    ///
    /// # Errors
    /// [`PlatformError::Unsupported`] when the platform has no threads (use the inline
    /// scheduler there), or the error of the thread port when a thread cannot be created.
    pub fn new(workers: u32, threads: &dyn IThreads) -> Result<Self, PlatformError> {
        let crowded = workers > threads.parallelism();
        let policy = if crowded {
            WaitPolicy::PARK
        } else {
            WaitPolicy::DEFAULT
        };
        Self::with_policy(workers, policy, threads)
    }

    /// As [`new`](Self::new) with an explicit wait policy (benchmarks and tests).
    ///
    /// # Errors
    /// As [`new`](Self::new).
    pub fn with_policy(
        workers: u32,
        policy: WaitPolicy,
        threads: &dyn IThreads,
    ) -> Result<Self, PlatformError> {
        if !threads.has_threads() {
            return Err(PlatformError::Unsupported);
        }
        let workers = Self::effective_workers(workers);
        let shared = Arc::new(Shared {
            deques: (0..workers).map(|_| Deque::new()).collect(),
            published: Published::new(),
            latch: Latch::new(),
            parker: Parker::new(),
            busy: AtomicBool::new(false),
            policy,
            clock: StdClock::new(),
        });
        let mut pool = Self {
            shared,
            workers,
            threads: Vec::with_capacity(workers as usize - 1),
        };
        for number in 1..workers {
            // On failure `pool` is dropped here: it closes the parker and joins the threads
            // already started.
            pool.start_thread(threads, number)?;
        }
        Ok(pool)
    }

    /// The number of workers a request of `workers` gets: at least 1, at most `MAX_WORKERS`.
    fn effective_workers(workers: u32) -> u32 {
        workers.clamp(1, Self::MAX_WORKERS)
    }

    fn start_thread(&mut self, threads: &dyn IThreads, number: u32) -> Result<(), PlatformError> {
        let shared = Arc::clone(&self.shared);
        let job: ThreadJob = Box::new(move || worker_main(&shared, number as usize));
        self.threads
            .push(threads.spawn(&format!("duck-job-{number}"), job)?);
        Ok(())
    }

    /// The wait policy of this pool.
    #[must_use]
    pub fn wait_policy(&self) -> WaitPolicy {
        self.shared.policy
    }
}

impl Drop for WorkStealingPool {
    fn drop(&mut self) {
        self.shared.parker.close();
        for handle in self.threads.drain(..) {
            // A failed join only means the worker panicked; the build aborts on panic.
            let _ = handle.join();
        }
    }
}

impl IScheduler for WorkStealingPool {
    fn worker_count(&self) -> u32 {
        self.workers
    }

    fn parallel_for(&self, count: u32, grain: u32, body: &RangeBody<'_>) -> Result<(), JobError> {
        let job = Job::for_loop(count, grain, body)?;
        self.shared.run(&job, self.workers > 1)
    }

    fn run_scope(&self, tasks: &[&Task<'_>]) -> Result<(), JobError> {
        let job = Job::scope(tasks)?;
        self.shared.run(&job, self.workers > 1)
    }
}
