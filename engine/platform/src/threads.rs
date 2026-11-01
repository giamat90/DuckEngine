//! The thread port: create a joinable thread, ask how many run at once.
//!
//! Mutexes, condvars, atomics and thread-local storage are `std` directly; only thread creation
//! goes through the port. [`NullThreads`] is the single-thread path (`Caps::has_threads` false):
//! `spawn` returns [`PlatformError::Unsupported`] and callers run the work inline.

use crate::error::PlatformError;
use std::thread::JoinHandle;

/// The work of a spawned thread.
pub type Job = Box<dyn FnOnce() + Send + 'static>;

/// A running thread that can be joined.
#[derive(Debug)]
pub struct ThreadHandle {
    inner: JoinHandle<()>,
}

impl ThreadHandle {
    /// Waits for the thread to end.
    ///
    /// # Errors
    /// [`PlatformError::Io`] when the job panicked (the build aborts on panic, so this only shows
    /// in builds that unwind; a job panic and an OS failure share this value on purpose in v0).
    pub fn join(self) -> Result<(), PlatformError> {
        self.inner.join().map_err(|_| PlatformError::Io)
    }
}

/// Thread creation; see the module documentation.
pub trait IThreads: Send + Sync {
    /// Starts `job` on a new thread called `name`.
    ///
    /// # Errors
    /// [`PlatformError::Unsupported`] when the platform has no threads, [`PlatformError::Io`]
    /// when the system cannot create one.
    fn spawn(&self, name: &str, job: Job) -> Result<ThreadHandle, PlatformError>;

    /// How many threads can run at once; at least 1. Cheap: safe to call every frame.
    fn parallelism(&self) -> u32;

    /// True when [`spawn`](Self::spawn) can create threads.
    fn has_threads(&self) -> bool;
}

/// Threads of the operating system, through `std::thread`. The parallelism is read once at
/// construction, because reading it can allocate and make system calls.
#[derive(Clone, Copy, Debug)]
pub struct StdThreads {
    parallelism: u32,
}

impl StdThreads {
    /// Threads of the operating system.
    #[must_use]
    pub fn new() -> Self {
        let parallelism = std::thread::available_parallelism()
            .map_or(1, |n| u32::try_from(n.get()).unwrap_or(u32::MAX));
        Self { parallelism }
    }
}

impl Default for StdThreads {
    fn default() -> Self {
        Self::new()
    }
}

impl IThreads for StdThreads {
    fn spawn(&self, name: &str, job: Job) -> Result<ThreadHandle, PlatformError> {
        if name.contains('\0') {
            return Err(PlatformError::InvalidPath);
        }
        std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(job)
            .map(|inner| ThreadHandle { inner })
            .map_err(|_| PlatformError::Io)
    }

    fn parallelism(&self) -> u32 {
        self.parallelism
    }

    fn has_threads(&self) -> bool {
        true
    }
}

/// The Null implementation: no threads at all.
#[derive(Clone, Copy, Debug, Default)]
pub struct NullThreads;

impl IThreads for NullThreads {
    fn spawn(&self, _name: &str, _job: Job) -> Result<ThreadHandle, PlatformError> {
        Err(PlatformError::Unsupported)
    }

    fn parallelism(&self) -> u32 {
        1
    }

    fn has_threads(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract;

    #[test]
    fn std_threads_pass_the_contract() {
        contract::threads_contract(&StdThreads::new(), true);
    }

    #[test]
    fn null_threads_pass_the_contract() {
        contract::threads_contract(&NullThreads, false);
    }

    #[test]
    fn std_threads_report_the_parallelism_of_the_system() {
        let system = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        let threads = StdThreads::new();
        assert_eq!(threads.parallelism() as usize, system);
        assert!(threads.has_threads());
    }

    #[test]
    fn null_threads_report_one_thread_and_no_spawning() {
        assert_eq!(NullThreads.parallelism(), 1);
        assert!(!NullThreads.has_threads());
    }

    #[test]
    fn a_spawned_thread_carries_its_name() {
        let (tx, rx) = std::sync::mpsc::channel();
        let job = Box::new(move || {
            let _ = tx.send(std::thread::current().name().map(str::to_owned));
        });
        let handle = StdThreads::new().spawn("named-job", job);
        assert_eq!(handle.map(ThreadHandle::join), Ok(Ok(())));
        assert_eq!(rx.recv(), Ok(Some("named-job".to_owned())));
    }

    #[test]
    fn a_name_with_a_nul_byte_is_refused_not_a_panic() {
        let spawned = StdThreads::new().spawn("bad\0name", Box::new(|| {}));
        assert_eq!(spawned.err(), Some(PlatformError::InvalidPath));
    }
}
