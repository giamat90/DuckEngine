//! The scaling scene: the W1 pyramid solved by the real work-stealing pool at 1, 2, 4 and 6
//! workers (owner choice O10: the reference machine has 6 performance cores, so a 20-thread
//! figure would mean nothing), once with the scalar solver and once with the lane path.
//! Parallel efficiency is the time at one worker over N times the time at N workers, within the
//! same solver path.

use crate::error::BenchError;
use crate::measure::measure_w1;
use crate::stats::Summary;
use jobs::pool::WorkStealingPool;
use jobs::scheduler::IScheduler;
use physics::d2::{DEFAULT_CHUNK, SolverExecution};
use platform::threads::StdThreads;
use std::sync::Arc;

/// The worker counts of the scaling scene.
pub const WORKER_COUNTS: [u32; 4] = [1, 2, 4, 6];

/// One line of the scaling table.
#[derive(Debug, Clone)]
pub struct ScalingRow {
    /// A short description, for example `pool 4, lanes`.
    pub label: String,
    /// Workers of the pool, `None` for the serial solver on the calling thread.
    pub workers: Option<u32>,
    /// True for the four-wide lane path.
    pub simd: bool,
    /// Nanoseconds per step.
    pub time: Summary,
    /// The state hash after the last step; every row must agree (D0 and D1).
    pub hash: u64,
    /// Time at one worker over N times the time at N workers, same solver path; `None` for the
    /// serial rows.
    pub efficiency: Option<f64>,
}

/// Time at one worker over N times the time at N workers; zero when either is not positive.
#[must_use]
pub fn parallel_efficiency(one_worker: f64, n_workers: f64, workers: u32) -> f64 {
    if n_workers <= 0.0 || workers == 0 {
        return 0.0;
    }
    one_worker / (f64::from(workers) * n_workers)
}

/// A work-stealing pool of `workers` threads, the caller included.
///
/// # Errors
/// [`BenchError::Pool`] when a thread cannot be created.
pub fn make_pool(workers: u32) -> Result<Arc<dyn IScheduler>, BenchError> {
    let pool = WorkStealingPool::new(workers, &StdThreads::new())?;
    Ok(Arc::new(pool))
}

fn execution(scheduler: Option<Arc<dyn IScheduler>>, simd: bool) -> SolverExecution {
    SolverExecution {
        scheduler,
        simd,
        chunk: DEFAULT_CHUNK,
    }
}

/// The path name in a label.
fn path(simd: bool) -> &'static str {
    if simd { "lanes" } else { "scalar" }
}

fn measure_row(
    rows: u8,
    steps: u32,
    iterations: u32,
    workers: Option<u32>,
    simd: bool,
) -> Result<ScalingRow, BenchError> {
    let scheduler = workers.map(make_pool).transpose()?;
    let measured = measure_w1(rows, steps, iterations, &execution(scheduler, simd), false)?;
    let label = match workers {
        Some(n) => format!("pool {n}, {}", path(simd)),
        None => format!("serial, {}", path(simd)),
    };
    Ok(ScalingRow {
        label,
        workers,
        simd,
        time: measured.time,
        hash: measured.run.hash,
        efficiency: None,
    })
}

/// Fills in the efficiency of every pool row against the one-worker row of its solver path.
fn add_efficiency(table: &mut [ScalingRow]) {
    let one_worker = |simd: bool, table: &[ScalingRow]| {
        table
            .iter()
            .find(|row| row.workers == Some(1) && row.simd == simd)
            .map(|row| row.time.median)
    };
    let baselines = [one_worker(false, table), one_worker(true, table)];
    for row in table.iter_mut() {
        let baseline = baselines[usize::from(row.simd)];
        row.efficiency = match (row.workers, baseline) {
            (Some(n), Some(t1)) => Some(parallel_efficiency(t1, row.time.median, n)),
            _ => None,
        };
    }
}

/// Measures the pyramid of `rows` rows for `steps` steps, `iterations` timed iterations per row:
/// the serial scalar reference, the serial lane path, then the pool at every count of
/// [`WORKER_COUNTS`] with the scalar and with the lane solver.
///
/// # Errors
/// An error of the pool or of the scene; [`BenchError::NothingToMeasure`] for a zero count.
pub fn run_scaling(rows: u8, steps: u32, iterations: u32) -> Result<Vec<ScalingRow>, BenchError> {
    let mut table = Vec::new();
    for (workers, simd) in configurations() {
        table.push(measure_row(rows, steps, iterations, workers, simd)?);
    }
    add_efficiency(&mut table);
    Ok(table)
}

/// The rows of the table, in order.
fn configurations() -> Vec<(Option<u32>, bool)> {
    let mut all = vec![(None, false), (None, true)];
    for simd in [false, true] {
        all.extend(WORKER_COUNTS.iter().map(|&n| (Some(n), simd)));
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rows_are_the_two_serial_paths_then_the_pool_per_path() {
        let all = configurations();
        assert_eq!(all.len(), 10);
        assert_eq!(all.first(), Some(&(None, false)));
        assert_eq!(all.get(1), Some(&(None, true)));
        assert_eq!(all.get(2), Some(&(Some(1), false)));
        assert_eq!(all.last(), Some(&(Some(6), true)));
    }
}
