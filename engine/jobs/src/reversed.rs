//! The reversed test scheduler: runs every job on the calling thread, last one first.

use crate::error::JobError;
use crate::scheduler::{Chunks, IScheduler, RangeBody, Task, run_tasks};

/// Runs chunks from the last to the first and tasks from the last to the first. A body that gives
/// another result than under the inline scheduler depends on the order. Test tool.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReversedScheduler;

impl IScheduler for ReversedScheduler {
    fn worker_count(&self) -> u32 {
        1
    }

    fn parallel_for(&self, count: u32, grain: u32, body: &RangeBody<'_>) -> Result<(), JobError> {
        let chunks = Chunks::new(count, grain)?;
        chunks.run((0..chunks.len()).rev(), body);
        Ok(())
    }

    fn run_scope(&self, tasks: &[&Task<'_>]) -> Result<(), JobError> {
        run_tasks(tasks, (0..tasks.len()).rev());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{
        observed_chunk_order, observed_task_order, order_independence_contract, scheduler_contract,
    };
    use crate::inline::InlineScheduler;

    #[test]
    fn the_reversed_scheduler_passes_the_contract() {
        scheduler_contract(&ReversedScheduler);
    }

    #[test]
    fn chunks_run_from_the_last_to_the_first() {
        assert_eq!(
            observed_chunk_order(&ReversedScheduler, 10, 3),
            vec![9, 6, 3, 0]
        );
    }

    #[test]
    fn tasks_run_from_the_last_to_the_first() {
        assert_eq!(observed_task_order(&ReversedScheduler, 4), vec![3, 2, 1, 0]);
    }

    #[test]
    fn a_pure_loop_gives_the_inline_result() {
        order_independence_contract(&[&InlineScheduler, &ReversedScheduler]);
    }
}
