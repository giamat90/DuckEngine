//! Candidates (b) and (c): per-worker deques with stealing. One implementation, generic over
//! the deque, so the two differ in the deque only (a mutex-protected `VecDeque` against a
//! Chase-Lev deque of atomics).
//!
//! Distribution is lazy binary splitting, the way a work-stealing `parallel_for` is usually
//! built: the caller starts with the range of all items; whoever holds a range of more than one
//! item pushes its upper half on its own deque and keeps splitting the lower half until one item
//! is left, which it runs. Idle workers steal the oldest (largest) range of another worker.
//! Every push and pop is by the deque's owner, which the Chase-Lev protocol requires, and no
//! chunk count limit exists (a deque holds at most one half per level of splitting).

use crate::deques::{Deque, pack, unpack};
use crate::erased::Published;
use crate::job::Job;
use crate::sync::{Latch, Parker};
use crate::workers::Workers;
use jobs::error::JobError;
use jobs::scheduler::{IScheduler, RangeBody, Task};
use platform::error::PlatformError;
use platform::threads::{IThreads, Job as ThreadJob};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Failed scans (every other deque tried once) a worker survives before it leaves a call; the
/// first `SPIN_SCANS` spin, the rest yield the time slice. The caller has no patience: it goes
/// straight to the latch.
const IDLE_SCANS: u32 = 128;
const SPIN_SCANS: u32 = 64;

/// What one worker has done and holds inside one call.
struct WorkState {
    /// Items run and not yet counted on the latch.
    executed: u32,
    /// Failed scans in a row.
    idle: u32,
    /// The range to start with (the caller's, for the caller).
    next: Option<u64>,
}

struct Shared<D> {
    deques: Vec<D>,
    published: Published,
    latch: Latch,
    parker: Parker,
    busy: AtomicBool,
}

fn backoff(idle: u32) {
    if idle > SPIN_SCANS {
        std::thread::yield_now();
    } else {
        std::hint::spin_loop();
    }
}

impl<D: Deque> Shared<D> {
    fn steal(&self, me: usize) -> Option<u64> {
        let count = self.deques.len();
        (1..count).find_map(|offset| self.deques[(me + offset) % count].steal())
    }

    fn run_item(&self, index: u32) {
        // SAFETY: the item belongs to a range taken from a deque (or is the caller's own) and
        // is not counted on the latch yet, so the call cannot have returned.
        let current = unsafe { self.published.current() };
        if let Some(job) = current {
            job.run_item(index);
        }
    }

    /// Runs the items `lo..hi` one after another; returns how many.
    fn run_all(&self, lo: u32, hi: u32) -> u32 {
        for index in lo..hi {
            self.run_item(index);
        }
        hi.saturating_sub(lo)
    }

    /// Pushes the upper half of `lo..hi` and returns the new end and how many items it had to
    /// run itself because the deque was full.
    fn split_once(&self, me: usize, lo: u32, hi: u32) -> (u32, u32) {
        let mid = lo + (hi - lo) / 2;
        let pushed = self.deques[me].push(pack(mid, hi));
        (mid, if pushed { 0 } else { self.run_all(mid, hi) })
    }

    /// Splits the range `item` down to one item and runs that; returns how many items this
    /// call ran (one, or more when a deque was full).
    fn run_range(&self, me: usize, item: u64) -> u32 {
        let (lo, mut hi) = unpack(item);
        let mut ran = 0;
        while hi - lo > 1 {
            let (end, extra) = self.split_once(me, lo, hi);
            hi = end;
            ran += extra;
        }
        ran + self.run_all(lo, hi)
    }

    /// One step of a worker inside a call: take an item (own deque first, then steal) and run
    /// it, or count what was run on the latch and back off. False when the worker should leave.
    fn step(&self, me: usize, state: &mut WorkState, patience: u32) -> bool {
        let item = state
            .next
            .take()
            .or_else(|| self.deques[me].pop())
            .or_else(|| self.steal(me));
        if let Some(item) = item {
            state.idle = 0;
            state.executed += self.run_range(me, item);
            return true;
        }
        if state.executed > 0 {
            self.latch.done(state.executed);
            state.executed = 0;
        }
        state.idle += 1;
        backoff(state.idle);
        state.idle <= patience
    }

    /// The loop of a worker inside a call; `patience` is how many failed scans it survives.
    fn work(&self, me: usize, first: Option<u64>, patience: u32) {
        let mut state = WorkState {
            executed: 0,
            idle: 0,
            next: first,
        };
        while self.step(me, &mut state, patience) {}
    }

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
        if wake && total > 1 {
            self.parker.wake_all();
        }
        self.work(0, Some(pack(0, total)), 0);
        self.latch.wait();
        self.published.clear();
        self.busy.store(false, Ordering::Release);
        Ok(())
    }
}

fn worker_main<D: Deque>(shared: &Shared<D>, me: usize) {
    let mut seen = 0;
    while shared.parker.wait(&mut seen) {
        shared.work(me, None, IDLE_SCANS);
    }
}

fn worker_job<D: Deque>(shared: &Arc<Shared<D>>, number: u32) -> ThreadJob {
    let shared = Arc::clone(shared);
    Box::new(move || worker_main(&shared, number as usize))
}

/// A pool of per-worker deques with stealing.
pub struct StealingScheduler<D: Deque> {
    shared: Arc<Shared<D>>,
    workers: u32,
    threads: Workers,
}

impl<D: Deque> StealingScheduler<D> {
    /// A pool of `workers` threads at the same time, the calling thread included. A worker
    /// spins `idle_spin` iterations for the next call before it parks.
    ///
    /// # Errors
    /// The error of the thread port when the threads cannot be created.
    pub fn new(
        workers: u32,
        idle_spin: u32,
        threads: &dyn IThreads,
    ) -> Result<Self, PlatformError> {
        let workers = workers.max(1);
        let shared = Arc::new(Shared {
            deques: (0..workers).map(|_| D::default()).collect(),
            published: Published::new(),
            latch: Latch::new(),
            parker: Parker::new(idle_spin),
            busy: AtomicBool::new(false),
        });
        let threads = Workers::start(
            threads,
            workers - 1,
            |number| worker_job(&shared, number),
            || shared.parker.close(),
        )?;
        Ok(Self {
            shared,
            workers,
            threads,
        })
    }
}

impl<D: Deque> Drop for StealingScheduler<D> {
    fn drop(&mut self) {
        self.shared.parker.close();
        self.threads.join();
    }
}

impl<D: Deque> IScheduler for StealingScheduler<D> {
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
