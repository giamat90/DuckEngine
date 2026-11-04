//! The only `unsafe` of the crate: the lifetime erasure of the borrowed job, and the guard that
//! makes the erasure sound on a build that unwinds.
//!
//! A pool hands a borrowed body to threads that outlive the call; the join of the port is what
//! makes that sound (design note section 8, ADR-0004 point 8). The protocol:
//!
//! 1. the caller publishes a pointer to its stack-held [`Job`] and arms the latch with the item
//!    count, before it pushes the first item (the push orders the publication before any steal);
//! 2. a thread runs an item only while it holds an item that is not yet counted as done on the
//!    latch, so the latch is not zero and the caller is still inside the call;
//! 3. the caller clears the pointer and returns (and the job dies) only after the latch reached
//!    zero;
//! 4. no thread unwinds out of a job: [`AbortOnUnwind`] ends the process instead, because an
//!    unwinding caller would free the job under the workers and an unwinding worker would leave
//!    the latch above zero forever. The workspace builds with `panic = abort`, where this guard
//!    never fires; it is for `cargo test`, which unwinds.
#![expect(
    unsafe_code,
    reason = "lifetime erasure of the borrowed job, one module, protocol in the module comment"
)]

use super::job::Job;
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};

// The pointer is shared between threads, so the job behind it must be `Sync` (an `AtomicPtr` is
// `Sync` whatever it points to, so this is checked here).
const _: fn() = || {
    fn assert_sync<T: Sync>() {}
    assert_sync::<Job<'static>>();
};

/// The pointer to the job of the call in progress.
pub struct Published {
    job: AtomicPtr<Job<'static>>,
}

impl Published {
    /// No job.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            job: AtomicPtr::new(ptr::null_mut()),
        }
    }

    /// Makes `job` the current job. Storing a pointer is safe; running through it is
    /// [`run_item`](Self::run_item).
    pub fn publish(&self, job: &Job<'_>) {
        let erased = ptr::from_ref(job).cast::<Job<'static>>().cast_mut();
        self.job.store(erased, Ordering::Release);
    }

    /// Forgets the job (the call is over).
    pub fn clear(&self) {
        self.job.store(ptr::null_mut(), Ordering::Release);
    }

    /// Runs item `index` of the current job (nothing when no job is published).
    ///
    /// This is a safe function that rests on a protocol of the pool, which the types cannot
    /// express and which only the pool's work loop uses (the module is private to the pool): the
    /// caller holds an item of the current call that is not yet counted as done on the call's
    /// latch (point 2 of the module comment), so the call cannot return, and the job published
    /// by it is alive, for the whole run of this function.
    pub fn run_item(&self, index: u32) {
        let job = self.job.load(Ordering::Acquire);
        // SAFETY: `job` is null or points to the job of the call in progress. The caller of this
        // function holds an uncounted item of that call (see the doc comment), so the latch is
        // above zero, the call has not cleared the pointer or returned, and the job is alive for
        // the whole use of the reference, which only reads it (the job holds shared references).
        if let Some(job) = unsafe { job.as_ref() } {
            job.run_item(index);
        }
    }
}

impl Default for Published {
    fn default() -> Self {
        Self::new()
    }
}

/// Ends the process when dropped during unwinding (point 4 of the module comment). Create it at
/// the top of every function that runs jobs and let it drop at the end.
pub struct AbortOnUnwind;

impl Drop for AbortOnUnwind {
    fn drop(&mut self) {
        if std::thread::panicking() {
            platform::process::abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::Task;
    use std::ops::Range;
    use std::sync::atomic::AtomicU32;

    #[test]
    fn nothing_runs_without_a_published_job() {
        Published::new().run_item(0);
    }

    #[test]
    fn a_published_job_runs_its_item_until_it_is_cleared() {
        let hits = AtomicU32::new(0);
        let body = |range: Range<u32>| {
            hits.fetch_add(range.end - range.start, Ordering::Relaxed);
        };
        let job = Job::for_loop(10, 5, &body).unwrap_or_else(|e| panic!("valid loop: {e}"));
        let published = Published::default();
        published.publish(&job);
        published.run_item(0);
        published.run_item(1);
        assert_eq!(hits.load(Ordering::Relaxed), 10);
        published.clear();
        published.run_item(0);
        assert_eq!(hits.load(Ordering::Relaxed), 10, "a cleared job is not run");
    }

    #[test]
    fn a_published_scope_runs_its_task() {
        let hits = AtomicU32::new(0);
        let task = || {
            hits.fetch_add(1, Ordering::Relaxed);
        };
        let tasks: [&Task<'_>; 1] = [&task];
        let job = Job::scope(&tasks).unwrap_or_else(|e| panic!("valid scope: {e}"));
        let published = Published::new();
        published.publish(&job);
        published.run_item(0);
        assert_eq!(hits.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn the_guard_does_nothing_without_a_panic() {
        let _guard = AbortOnUnwind;
    }
}
