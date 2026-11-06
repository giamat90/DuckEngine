//! Scheduler overhead: what one fork-join call costs when the job is empty or tiny, on the inline
//! scheduler (no threads: the floor) and on the work-stealing pool. The solver makes one such call
//! per colour per sub-step pass, so this is the price of each barrier.

use crate::error::BenchError;
use crate::scaling::{WORKER_COUNTS, make_pool};
use crate::scene::NANOS_PER_SECOND;
use crate::stats::{Summary, summarize};
use jobs::inline::InlineScheduler;
use jobs::scheduler::IScheduler;
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Indices of the tiny job.
pub const TINY_COUNT: u32 = 64;

/// Indices per chunk of the tiny job: eight chunks.
pub const TINY_GRAIN: u32 = 8;

/// The kind of job a call forks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverheadKind {
    /// One index, an empty body: the cost of the call itself.
    Empty,
    /// 64 indices in chunks of 8, each adding its index to a counter: a few hundred instructions
    /// spread over several chunks.
    Tiny,
}

/// One line of the overhead table.
#[derive(Debug, Clone, PartialEq)]
pub struct OverheadRow {
    /// Empty or tiny job.
    pub kind: OverheadKind,
    /// Workers of the pool, `None` for the inline scheduler.
    pub workers: Option<u32>,
    /// Nanoseconds per call: minimum, median and 95th percentile over the repetitions.
    pub time: Summary,
}

/// One call of the job of `kind`.
fn call(
    scheduler: &dyn IScheduler,
    kind: OverheadKind,
    sink: &AtomicU64,
) -> Result<(), BenchError> {
    let result = match kind {
        OverheadKind::Empty => scheduler.parallel_for(1, 1, &|range| {
            black_box(range);
        }),
        OverheadKind::Tiny => scheduler.parallel_for(TINY_COUNT, TINY_GRAIN, &|range| {
            let sum: u64 = range.map(u64::from).sum();
            sink.fetch_add(sum, Ordering::Relaxed);
        }),
    };
    result.map_err(|_| BenchError::Scheduler)
}

/// The sum the tiny job leaves in its counter after one call on the inline scheduler: the sum of
/// the indices below [`TINY_COUNT`] when every index runs exactly once.
#[must_use]
pub fn tiny_job_checksum() -> u64 {
    let sink = AtomicU64::new(0);
    match call(&InlineScheduler, OverheadKind::Tiny, &sink) {
        Ok(()) => sink.load(Ordering::Relaxed),
        Err(_) => 0,
    }
}

/// Nanoseconds per call over `reps` repetitions of `calls` calls (one more repetition first,
/// discarded, as warm-up).
fn time_calls(
    scheduler: &dyn IScheduler,
    kind: OverheadKind,
    calls: u32,
    reps: u32,
) -> Result<Summary, BenchError> {
    let sink = AtomicU64::new(0);
    let mut samples = Vec::new();
    for rep in 0..=reps {
        let began = Instant::now();
        for _ in 0..calls {
            call(scheduler, kind, &sink)?;
        }
        if rep > 0 {
            samples.push(began.elapsed().as_secs_f64() * NANOS_PER_SECOND / f64::from(calls));
        }
    }
    black_box(sink.load(Ordering::Relaxed));
    summarize(&mut samples).ok_or(BenchError::NothingToMeasure)
}

/// Measures both kinds of job on the inline scheduler and on the pool at every count of
/// [`WORKER_COUNTS`]: `calls` calls per repetition, `reps` timed repetitions.
///
/// # Errors
/// [`BenchError::NothingToMeasure`] for a zero count, or an error of the pool.
pub fn run_overhead(calls: u32, reps: u32) -> Result<Vec<OverheadRow>, BenchError> {
    if calls == 0 || reps == 0 {
        return Err(BenchError::NothingToMeasure);
    }
    let mut rows = Vec::new();
    for kind in [OverheadKind::Empty, OverheadKind::Tiny] {
        let time = time_calls(&InlineScheduler, kind, calls, reps)?;
        rows.push(OverheadRow {
            kind,
            workers: None,
            time,
        });
        for workers in WORKER_COUNTS {
            let pool = make_pool(workers)?;
            let time = time_calls(pool.as_ref(), kind, calls, reps)?;
            rows.push(OverheadRow {
                kind,
                workers: Some(workers),
                time,
            });
        }
    }
    Ok(rows)
}
