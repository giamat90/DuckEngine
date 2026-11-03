//! The inline scheduler: runs every job on the calling thread, in ascending order.

use crate::error::JobError;
use crate::scheduler::{Chunks, IScheduler, RangeBody, Task, run_tasks};

/// Runs jobs on the calling thread. Chunks run in ascending order and tasks in slice order, so
/// the run is fully deterministic. It is the Null path: used when `Caps::has_threads` is false
/// and as the reference in tests.
#[derive(Clone, Copy, Debug, Default)]
pub struct InlineScheduler;

impl IScheduler for InlineScheduler {
    fn worker_count(&self) -> u32 {
        1
    }

    fn parallel_for(&self, count: u32, grain: u32, body: &RangeBody<'_>) -> Result<(), JobError> {
        let chunks = Chunks::new(count, grain)?;
        chunks.run(0..chunks.len(), body);
        Ok(())
    }

    fn run_scope(&self, tasks: &[&Task<'_>]) -> Result<(), JobError> {
        run_tasks(tasks, 0..tasks.len());
        Ok(())
    }
}
