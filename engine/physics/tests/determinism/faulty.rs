//! Schedulers that are correct except for one deliberate fault. The harness must fail on every one
//! of them (lesson R-23: a check that cannot fail proves nothing): once through the scheduler
//! contract suite, and once through the replay comparison on a scene whose colours go to the
//! scheduler. The faults are made in test code only; no production code has a fault switch.
//! A scheduler that returns before the jobs are done cannot be written in safe code (the body is
//! borrowed), so that fault is covered by the pool tests of `jobs`, not here.

use jobs::error::JobError;
use jobs::scheduler::{IScheduler, RangeBody, Task};
use std::ops::Range;

/// The fault of a scheduler.
#[derive(Clone, Copy, Debug)]
pub enum Fault {
    /// Skips the last chunk.
    SkipsLastChunk,
    /// Runs the first chunk twice.
    RunsFirstChunkTwice,
    /// Runs nothing and reports success.
    RunsNothing,
    /// Skips the last task of a scope.
    SkipsLastTask,
    /// Runs the first task of a scope twice.
    RunsFirstTaskTwice,
}

/// Every fault, for the contract test that must catch each one.
pub const ALL: [Fault; 5] = [
    Fault::SkipsLastChunk,
    Fault::RunsFirstChunkTwice,
    Fault::RunsNothing,
    Fault::SkipsLastTask,
    Fault::RunsFirstTaskTwice,
];

/// The faults that change the loops the solver hands out (`parallel_for`).
pub const LOOP_FAULTS: [Fault; 3] = [
    Fault::SkipsLastChunk,
    Fault::RunsFirstChunkTwice,
    Fault::RunsNothing,
];

/// A scheduler with one fault, running everything on the calling thread.
pub struct Faulty(pub Fault);

impl Faulty {
    fn ranges(&self, count: u32, grain: u32) -> Result<Vec<Range<u32>>, JobError> {
        if grain == 0 {
            return Err(JobError::InvalidGrain);
        }
        let mut ranges: Vec<Range<u32>> = (0..count)
            .step_by(grain as usize)
            .map(|start| start..start.saturating_add(grain).min(count))
            .collect();
        match self.0 {
            Fault::SkipsLastChunk => drop(ranges.pop()),
            Fault::RunsFirstChunkTwice => repeat_first(&mut ranges),
            Fault::RunsNothing => ranges.clear(),
            Fault::SkipsLastTask | Fault::RunsFirstTaskTwice => (),
        }
        Ok(ranges)
    }

    fn tasks(&self, len: usize) -> Vec<usize> {
        let mut order: Vec<usize> = (0..len).collect();
        match self.0 {
            Fault::SkipsLastTask => drop(order.pop()),
            Fault::RunsFirstTaskTwice => repeat_first(&mut order),
            Fault::RunsNothing => order.clear(),
            Fault::SkipsLastChunk | Fault::RunsFirstChunkTwice => (),
        }
        order
    }
}

fn repeat_first<T: Clone>(items: &mut Vec<T>) {
    if let Some(first) = items.first().cloned() {
        items.insert(0, first);
    }
}

impl IScheduler for Faulty {
    fn worker_count(&self) -> u32 {
        1
    }

    fn parallel_for(&self, count: u32, grain: u32, body: &RangeBody<'_>) -> Result<(), JobError> {
        self.ranges(count, grain)?.into_iter().for_each(body);
        Ok(())
    }

    fn run_scope(&self, tasks: &[&Task<'_>]) -> Result<(), JobError> {
        self.tasks(tasks.len())
            .into_iter()
            .filter_map(|index| tasks.get(index))
            .for_each(|task| task());
        Ok(())
    }
}
