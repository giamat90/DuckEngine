//! Candidate (a): one shared queue behind a `Mutex` and a `Condvar`.
//!
//! The queue is the pair `next`/`total` of the call in progress: every item is one claim under
//! the lock (the textbook shared queue, one lock round trip per item; the claim could take a
//! batch, which would shrink the cost per item and is not done here). Idle workers wait on the
//! condvar, so this candidate has no spin mode.

use crate::erased::Published;
use crate::job::Job;
use crate::sync::{Latch, lock};
use crate::workers::Workers;
use jobs::error::JobError;
use jobs::scheduler::{IScheduler, RangeBody, Task};
use platform::error::PlatformError;
use platform::threads::{IThreads, Job as ThreadJob};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};

struct Queue {
    next: u32,
    total: u32,
    shutdown: bool,
}

struct Shared {
    queue: Mutex<Queue>,
    work: Condvar,
    published: Published,
    latch: Latch,
    busy: AtomicBool,
}

impl Shared {
    /// Takes the next item of the call in progress, if any.
    fn claim(&self) -> Option<u32> {
        let mut queue = lock(&self.queue);
        let item = queue.next;
        let has_item = item < queue.total;
        queue.next += u32::from(has_item);
        has_item.then_some(item)
    }

    fn run_item(&self, item: u32) {
        // SAFETY: the item is claimed and not yet counted on the latch, so the call that
        // published the job cannot have returned.
        let current = unsafe { self.published.current() };
        if let Some(job) = current {
            job.run_item(item);
        }
    }

    /// Runs items until the queue is empty, then counts them on the latch.
    fn drain(&self) {
        let mut executed = 0;
        while let Some(item) = self.claim() {
            self.run_item(item);
            executed += 1;
        }
        if executed > 0 {
            self.latch.done(executed);
        }
    }

    /// Parks until there is an item or the scheduler shuts down; false on shutdown.
    fn wait_for_work(&self) -> bool {
        let mut queue = lock(&self.queue);
        while queue.next >= queue.total && !queue.shutdown {
            queue = self
                .work
                .wait(queue)
                .unwrap_or_else(PoisonError::into_inner);
        }
        !queue.shutdown
    }

    fn close(&self) {
        lock(&self.queue).shutdown = true;
        self.work.notify_all();
    }

    fn fill(&self, total: u32) {
        let mut queue = lock(&self.queue);
        queue.next = 0;
        queue.total = total;
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
        self.fill(total);
        if wake && total > 1 {
            self.work.notify_all();
        }
        self.drain();
        self.latch.wait();
        self.published.clear();
        self.busy.store(false, Ordering::Release);
        Ok(())
    }
}

fn worker_main(shared: &Shared) {
    shared.drain();
    while shared.wait_for_work() {
        shared.drain();
    }
}

fn worker_job(shared: &Arc<Shared>) -> ThreadJob {
    let shared = Arc::clone(shared);
    Box::new(move || worker_main(&shared))
}

/// Candidate (a): a shared queue pool.
pub struct SharedQueueScheduler {
    shared: Arc<Shared>,
    workers: u32,
    threads: Workers,
}

impl SharedQueueScheduler {
    /// A pool of `workers` threads at the same time, the calling thread included.
    ///
    /// # Errors
    /// The error of the thread port when the threads cannot be created.
    pub fn new(workers: u32, threads: &dyn IThreads) -> Result<Self, PlatformError> {
        let workers = workers.max(1);
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                next: 0,
                total: 0,
                shutdown: false,
            }),
            work: Condvar::new(),
            published: Published::new(),
            latch: Latch::new(),
            busy: AtomicBool::new(false),
        });
        let threads = Workers::start(
            threads,
            workers - 1,
            |_| worker_job(&shared),
            || shared.close(),
        )?;
        Ok(Self {
            shared,
            workers,
            threads,
        })
    }
}

impl Drop for SharedQueueScheduler {
    fn drop(&mut self) {
        self.shared.close();
        self.threads.join();
    }
}

impl IScheduler for SharedQueueScheduler {
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
