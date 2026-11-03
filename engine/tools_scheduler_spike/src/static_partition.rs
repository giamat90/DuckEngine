//! Candidate (d): static partition. The items are cut into one contiguous block per worker when
//! the call starts; a worker runs its block and nothing else, there is no queue and no stealing.
//! Every call needs all workers (the caller waits for each of them), so its join latency is that
//! of the slowest wake-up; it has the least overhead per item and cannot rebalance.

use crate::erased::Published;
use crate::job::{Job, block_bounds};
use crate::sync::{Latch, Parker};
use crate::workers::Workers;
use jobs::error::JobError;
use jobs::scheduler::{IScheduler, RangeBody, Task};
use platform::error::PlatformError;
use platform::threads::{IThreads, Job as ThreadJob};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

struct Shared {
    published: Published,
    latch: Latch,
    parker: Parker,
    busy: AtomicBool,
    workers: u32,
}

impl Shared {
    fn run_block(&self, job: &Job<'_>, worker: u32) {
        let (lo, hi) = block_bounds(job.items(), self.workers, worker);
        for item in lo..hi {
            job.run_item(item);
        }
    }

    /// One worker's share of the call in progress.
    fn worker_turn(&self, worker: u32) {
        // SAFETY: the caller of this call waits for every worker's acknowledgement
        // (`latch.done(1)` below), which comes after this use, so the job is alive.
        let current = unsafe { self.published.current() };
        if let Some(job) = current {
            self.run_block(job, worker);
        }
        self.latch.done(1);
    }

    /// One item cannot be shared, and one worker has the whole loop: run it here.
    fn run_serial(job: &Job<'_>) {
        for item in 0..job.items() {
            job.run_item(item);
        }
    }

    fn run_shared(&self, job: &Job<'_>) {
        self.published.publish(job);
        self.latch.arm(self.workers - 1);
        self.parker.wake_all();
        self.run_block(job, 0);
        self.latch.wait();
        self.published.clear();
    }

    fn run(&self, job: &Job<'_>) -> Result<(), JobError> {
        let total = job.items();
        if total == 0 {
            return Ok(());
        }
        if self.busy.swap(true, Ordering::Acquire) {
            return Err(JobError::Busy);
        }
        if total == 1 || self.workers == 1 {
            Self::run_serial(job);
        } else {
            self.run_shared(job);
        }
        self.busy.store(false, Ordering::Release);
        Ok(())
    }
}

fn worker_main(shared: &Shared, number: u32) {
    let mut seen = 0;
    while shared.parker.wait(&mut seen) {
        shared.worker_turn(number);
    }
}

fn worker_job(shared: &Arc<Shared>, number: u32) -> ThreadJob {
    let shared = Arc::clone(shared);
    Box::new(move || worker_main(&shared, number))
}

/// Candidate (d): a static partition pool.
pub struct StaticPartitionScheduler {
    shared: Arc<Shared>,
    workers: u32,
    threads: Workers,
}

impl StaticPartitionScheduler {
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
            published: Published::new(),
            latch: Latch::new(),
            parker: Parker::new(idle_spin),
            busy: AtomicBool::new(false),
            workers,
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

impl Drop for StaticPartitionScheduler {
    fn drop(&mut self) {
        self.shared.parker.close();
        self.threads.join();
    }
}

impl IScheduler for StaticPartitionScheduler {
    fn worker_count(&self) -> u32 {
        self.workers
    }

    fn parallel_for(&self, count: u32, grain: u32, body: &RangeBody<'_>) -> Result<(), JobError> {
        let job = Job::for_loop(count, grain, body)?;
        self.shared.run(&job)
    }

    fn run_scope(&self, tasks: &[&Task<'_>]) -> Result<(), JobError> {
        let job = Job::scope(tasks)?;
        self.shared.run(&job)
    }
}
