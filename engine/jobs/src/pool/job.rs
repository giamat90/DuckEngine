//! One call of the port as data: a loop over chunks or a slice of tasks, both reduced to "run
//! item `i`", so the pool only distributes item numbers.

use crate::error::JobError;
use crate::scheduler::{RangeBody, Task};

/// What one `parallel_for` or `run_scope` call asks for.
pub enum Job<'a> {
    /// `count` indices cut into chunks of `grain`; item `i` is the chunk `i`.
    Loop {
        /// The loop body.
        body: &'a RangeBody<'a>,
        /// Number of indices.
        count: u32,
        /// Chunk size (at least 1).
        grain: u32,
    },
    /// Independent tasks; item `i` is task `i`.
    Scope {
        /// The tasks.
        tasks: &'a [&'a Task<'a>],
        /// Their number (checked to fit `u32`).
        len: u32,
    },
}

impl<'a> Job<'a> {
    /// A loop call.
    ///
    /// # Errors
    /// [`JobError::InvalidGrain`] for a zero grain.
    pub fn for_loop(count: u32, grain: u32, body: &'a RangeBody<'a>) -> Result<Self, JobError> {
        if grain == 0 {
            return Err(JobError::InvalidGrain);
        }
        Ok(Self::Loop { body, count, grain })
    }

    /// A scope call.
    ///
    /// # Errors
    /// [`JobError::TooManyChunks`] for more than `u32::MAX` tasks.
    pub fn scope(tasks: &'a [&'a Task<'a>]) -> Result<Self, JobError> {
        let len =
            u32::try_from(tasks.len()).map_err(|_| JobError::TooManyChunks { max: u32::MAX })?;
        Ok(Self::Scope { tasks, len })
    }

    /// How many items (chunks or tasks) the call has.
    #[must_use]
    pub fn items(&self) -> u32 {
        match self {
            Self::Loop { count, grain, .. } => count.div_ceil(*grain),
            Self::Scope { len, .. } => *len,
        }
    }

    /// Runs item `index` (ignored past the last item).
    pub fn run_item(&self, index: u32) {
        match self {
            Self::Loop { body, count, grain } => run_chunk(*body, *count, *grain, index),
            Self::Scope { tasks, .. } => run_task(tasks, index),
        }
    }
}

fn run_chunk(body: &RangeBody<'_>, count: u32, grain: u32, index: u32) {
    // index < items, so index * grain < count and cannot overflow.
    let start = index.saturating_mul(grain);
    if start < count {
        body(start..start.saturating_add(grain).min(count));
    }
}

fn run_task(tasks: &[&Task<'_>], index: u32) {
    if let Some(task) = tasks.get(index as usize) {
        task();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ops::Range;
    use std::sync::Mutex;

    #[test]
    fn a_loop_cuts_the_last_chunk_short_and_a_scope_counts_its_tasks() {
        let body = |_: Range<u32>| {};
        let job = Job::for_loop(10, 4, &body);
        assert_eq!(job.map(|job| job.items()), Ok(3));
        let one = || {};
        let tasks: [&Task<'_>; 2] = [&one, &one];
        assert_eq!(Job::scope(&tasks).map(|job| job.items()), Ok(2));
        let empty = Job::for_loop(0, 4, &body);
        assert_eq!(empty.map(|job| job.items()), Ok(0));
    }

    #[test]
    fn an_item_past_the_last_chunk_runs_nothing() {
        let calls = std::sync::atomic::AtomicU32::new(0);
        let body = |_: Range<u32>| {
            calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        };
        let job = Job::for_loop(10, 5, &body).unwrap_or_else(|e| panic!("valid loop: {e}"));
        job.run_item(2);
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 0);
    }

    #[test]
    fn a_zero_grain_is_refused() {
        let body = |_: Range<u32>| {};
        assert!(matches!(
            Job::for_loop(1, 0, &body),
            Err(JobError::InvalidGrain)
        ));
    }

    #[test]
    fn a_loop_item_runs_its_chunk() {
        let seen = Mutex::new(Vec::new());
        let body = |range: Range<u32>| {
            seen.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(range);
        };
        let job = Job::for_loop(10, 4, &body).unwrap_or_else(|e| panic!("valid loop: {e}"));
        job.run_item(2);
        job.run_item(0);
        job.run_item(3); // past the last item: ignored
        let seen = seen.into_inner().unwrap_or_default();
        assert_eq!(seen, vec![8..10, 0..4]);
    }

    #[test]
    fn a_scope_item_runs_its_task() {
        let log = Mutex::new(Vec::new());
        let push = |value: u32| {
            log.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(value);
        };
        let first = || push(1);
        let second = || push(2);
        let tasks: [&Task<'_>; 2] = [&first, &second];
        let job = Job::scope(&tasks).unwrap_or_else(|e| panic!("valid scope: {e}"));
        job.run_item(1);
        job.run_item(5); // past the last task: ignored
        job.run_item(0);
        assert_eq!(log.into_inner().unwrap_or_default(), vec![2, 1]);
    }
}
