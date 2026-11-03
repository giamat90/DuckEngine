//! One call of the port as data: a loop over chunks or a slice of tasks, both reduced to "run
//! item `i`" so the candidates only distribute item numbers.

use jobs::error::JobError;
use jobs::scheduler::{RangeBody, Task};

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

/// The first and last-plus-one item of block `block` when `items` items are cut into `blocks`
/// contiguous blocks (the static partition).
#[must_use]
pub fn block_bounds(items: u32, blocks: u32, block: u32) -> (u32, u32) {
    let edge = |b: u32| {
        let scaled = u64::from(items) * u64::from(b) / u64::from(blocks.max(1));
        u32::try_from(scaled).unwrap_or(items)
    };
    (edge(block), edge(block + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ITEMS: [u32; 5] = [0, 1, 5, 17, 1000];
    const BLOCKS: [u32; 5] = [1, 2, 3, 6, 20];

    /// True when the blocks follow each other without gap or overlap and end at `items`.
    fn tile(items: u32, blocks: u32) -> bool {
        let end = (0..blocks).try_fold(0, |next, block| {
            let (lo, hi) = block_bounds(items, blocks, block);
            (lo == next).then_some(hi)
        });
        end == Some(items)
    }

    fn tile_every_way(items: u32) -> bool {
        BLOCKS.iter().all(|blocks| tile(items, *blocks))
    }

    #[test]
    fn blocks_cover_the_items_without_overlap() {
        assert!(ITEMS.iter().all(|items| tile_every_way(*items)));
    }

    #[test]
    fn a_loop_cuts_the_last_chunk_short_and_a_scope_counts_its_tasks() {
        let body = |_: std::ops::Range<u32>| {};
        let job = Job::for_loop(10, 4, &body);
        assert_eq!(job.map(|job| job.items()).ok(), Some(3));
        let one = || {};
        let tasks: [&Task<'_>; 2] = [&one, &one];
        assert_eq!(Job::scope(&tasks).map(|job| job.items()).ok(), Some(2));
        assert!(matches!(
            Job::for_loop(1, 0, &body),
            Err(JobError::InvalidGrain)
        ));
    }
}
