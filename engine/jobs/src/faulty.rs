//! Test-only schedulers that are correct except for one deliberate fault, so each check of the
//! contract suite is shown to fail when it should (R-23: a contract helper can be emptied
//! without any test noticing).

use crate::error::JobError;
use crate::scheduler::{Chunks, IScheduler, RangeBody, Task, run_tasks};
use std::ops::Range;

#[derive(Clone, Copy, Debug)]
pub enum Fault {
    None,
    SkipsLastChunk,
    RunsFirstChunkTwice,
    AcceptsZeroGrain,
    SplitsTooFine,
    OverrunsTheRange,
    SkipsLastTask,
    RunsFirstTaskTwice,
    RefusesButRuns,
}

pub struct Faulty(pub Fault);

fn repeat_first<T: Clone>(items: &mut Vec<T>) {
    if let Some(first) = items.first().cloned() {
        items.insert(0, first);
    }
}

fn widen(range: &mut Range<u32>) {
    range.end += 1;
}

impl Faulty {
    fn grain_used(&self, grain: u32) -> u32 {
        match self.0 {
            Fault::AcceptsZeroGrain => grain.max(1),
            Fault::SplitsTooFine => 1,
            _ => grain,
        }
    }

    /// The ranges this scheduler calls the body with, faults included.
    fn planned_ranges(&self, count: u32, grain: u32) -> Result<Vec<Range<u32>>, JobError> {
        let chunks = Chunks::new(count, self.grain_used(grain))?;
        let mut ranges: Vec<Range<u32>> =
            (0..chunks.len()).filter_map(|i| chunks.range(i)).collect();
        match self.0 {
            Fault::SkipsLastChunk => drop(ranges.pop()),
            Fault::RunsFirstChunkTwice => repeat_first(&mut ranges),
            Fault::OverrunsTheRange => ranges.iter_mut().for_each(widen),
            _ => (),
        }
        Ok(ranges)
    }

    /// The refusal of a zero grain; the fault `RefusesButRuns` runs a job first.
    fn refuse(&self, body: &RangeBody<'_>) -> Result<(), JobError> {
        if matches!(self.0, Fault::RefusesButRuns) {
            body(0..1);
        }
        Err(JobError::InvalidGrain)
    }

    /// The task indices this scheduler runs, faults included.
    fn planned_tasks(&self, len: usize) -> Vec<usize> {
        let mut order: Vec<usize> = (0..len).collect();
        match self.0 {
            Fault::SkipsLastTask => drop(order.pop()),
            Fault::RunsFirstTaskTwice => repeat_first(&mut order),
            _ => (),
        }
        order
    }
}

impl IScheduler for Faulty {
    fn worker_count(&self) -> u32 {
        1
    }

    fn parallel_for(&self, count: u32, grain: u32, body: &RangeBody<'_>) -> Result<(), JobError> {
        if grain == 0 && !matches!(self.0, Fault::AcceptsZeroGrain) {
            return self.refuse(body);
        }
        self.planned_ranges(count, grain)?
            .into_iter()
            .for_each(body);
        Ok(())
    }

    fn run_scope(&self, tasks: &[&Task<'_>]) -> Result<(), JobError> {
        run_tasks(tasks, self.planned_tasks(tasks.len()).into_iter());
        Ok(())
    }
}

/// A correct scheduler that merges the whole loop into one chunk (allowed: one chunk covers
/// `0..count`), to show that per-index loops do not depend on chunk boundaries.
pub struct Merging;

impl IScheduler for Merging {
    fn worker_count(&self) -> u32 {
        1
    }

    fn parallel_for(&self, count: u32, grain: u32, body: &RangeBody<'_>) -> Result<(), JobError> {
        Chunks::new(count, grain)?;
        (count > 0).then(|| body(0..count));
        Ok(())
    }

    fn run_scope(&self, tasks: &[&Task<'_>]) -> Result<(), JobError> {
        run_tasks(tasks, 0..tasks.len());
        Ok(())
    }
}
