//! The measurement protocol of `docs/budgets/reference-machine-and-workloads.md`: one warm-up
//! pass that is discarded, then the timed iterations; the value reported is the median of the
//! pooled step times, the 95th percentile is recorded next to it.

use crate::error::BenchError;
use crate::scene::{SceneRun, run_scene};
use crate::stats::{Summary, summarize};
use physics::d2::SolverExecution;

/// Timed iterations the protocol asks for at least.
pub const MIN_ITERATIONS: u32 = 30;

/// The step times of the timed iterations of a scene, summarized, and the last run.
#[derive(Debug, Clone)]
pub struct Measured {
    /// Nanoseconds per step: minimum, median and 95th percentile of all `samples`.
    pub time: Summary,
    /// Step times pooled (iterations times steps).
    pub samples: usize,
    /// The last timed run: its counters, drift and hash (all the same in every run).
    pub run: SceneRun,
}

/// Runs the pyramid of `rows` rows for `steps` steps once as warm-up and `iterations` times
/// timed, each on a fresh world, and summarizes the step times.
///
/// # Errors
/// [`BenchError::NothingToMeasure`] when `iterations` or `steps` is zero, or an error of the scene.
pub fn measure_w1(
    rows: u8,
    steps: u32,
    iterations: u32,
    execution: &SolverExecution,
    sleeping: bool,
) -> Result<Measured, BenchError> {
    if iterations == 0 || steps == 0 {
        return Err(BenchError::NothingToMeasure);
    }
    run_scene(rows, steps, execution, sleeping)?;
    let mut samples = Vec::new();
    let mut last = None;
    for _ in 0..iterations {
        let mut run = run_scene(rows, steps, execution, sleeping)?;
        samples.append(&mut run.step_ns);
        last = Some(run);
    }
    let count = samples.len();
    match (summarize(&mut samples), last) {
        (Some(time), Some(run)) => Ok(Measured {
            time,
            samples: count,
            run,
        }),
        _ => Err(BenchError::NothingToMeasure),
    }
}
