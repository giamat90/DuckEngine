//! The only place that erases a lifetime. A pool hands a borrowed body to threads that outlive
//! the call; the join of the port is what makes that sound (design note section 8). The
//! protocol, the same for all four candidates:
//!
//! 1. the caller publishes a pointer to its stack-held [`Job`] and arms a latch with the item count;
//! 2. a thread dereferences the pointer only while it holds an item that is not yet counted as
//!    done, so the latch is not zero and the caller is still inside the call;
//! 3. the caller returns (and the job dies) only after the latch reached zero.

use crate::job::Job;
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};

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

    /// Makes `job` the current job. Storing a pointer is safe; using it is not.
    pub fn publish(&self, job: &Job<'_>) {
        let erased = ptr::from_ref(job).cast::<Job<'static>>().cast_mut();
        self.job.store(erased, Ordering::Release);
    }

    /// Forgets the job (the call is over).
    pub fn clear(&self) {
        self.job.store(ptr::null_mut(), Ordering::Release);
    }

    /// The current job.
    ///
    /// # Safety
    /// The caller must hold an item of the current call that is not yet counted as done on the
    /// call's latch (or have a reason of the same strength that the call cannot return), so the
    /// job published by that call is still alive for the whole use of the returned reference.
    #[must_use]
    pub unsafe fn current(&self) -> Option<&Job<'static>> {
        let job = self.job.load(Ordering::Acquire);
        // SAFETY: `job` is null or points to the job of the call in progress, which the caller of
        // this function guarantees is alive (see the `# Safety` section).
        unsafe { job.as_ref() }
    }
}

impl Default for Published {
    fn default() -> Self {
        Self::new()
    }
}
