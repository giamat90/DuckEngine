//! Worker threads, created through the platform `IThreads` seam (choice O5 of the design note).

use platform::error::PlatformError;
use platform::threads::{IThreads, Job as ThreadJob, ThreadHandle};

/// Joinable worker threads, joined by [`join`](Self::join) after the owner told them to stop.
pub struct Workers {
    handles: Vec<ThreadHandle>,
}

impl Workers {
    /// Starts `count` threads; thread `n` (1-based, the caller is worker 0) runs `make(n)`. When
    /// one cannot be created the ones already running are stopped with `stop` and joined.
    ///
    /// # Errors
    /// The error of the thread port (`Unsupported` without threads, `Io` when the system fails).
    pub fn start(
        threads: &dyn IThreads,
        count: u32,
        make: impl Fn(u32) -> ThreadJob,
        stop: impl Fn(),
    ) -> Result<Self, PlatformError> {
        let mut workers = Self {
            handles: Vec::with_capacity(count as usize),
        };
        let failed = (1..=count).find_map(|number| workers.start_one(threads, number, &make).err());
        if let Some(error) = failed {
            stop();
            workers.join();
            return Err(error);
        }
        Ok(workers)
    }

    fn start_one(
        &mut self,
        threads: &dyn IThreads,
        number: u32,
        make: &impl Fn(u32) -> ThreadJob,
    ) -> Result<(), PlatformError> {
        let handle = threads.spawn(&format!("sched-spike-{number}"), make(number))?;
        self.handles.push(handle);
        Ok(())
    }

    /// Waits for every thread to end.
    pub fn join(&mut self) {
        for handle in self.handles.drain(..) {
            // A failed join only means the job panicked; the build aborts on panic.
            let _ = handle.join();
        }
    }
}
