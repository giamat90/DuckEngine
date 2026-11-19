//! The timed measurement of a 3D scene (card 8-14): the same protocol as W1 (one warm-up run
//! discarded, then `iterations` timed runs, each on a fresh world, step times pooled) and the
//! scaling rows of a scene on the work-stealing pool.

use crate::error::BenchError;
use crate::scaling::{ScalingRow, scale_with};
use crate::scene3d::{Run3, Scene3, run_scene3};
use crate::stats::{Summary, summarize};
use physics::d3::SolverExecution;

/// The result of a timed measurement of a 3D scene.
#[derive(Debug, Clone)]
pub struct Measured3 {
    /// Nanoseconds per step: minimum, median and 95th percentile of all `samples`.
    pub time: Summary,
    /// Step times pooled (iterations times steps).
    pub samples: usize,
    /// The last timed run: drift, speed and hash (the same in every run).
    pub run: Run3,
}

/// Runs `scene` for `steps` steps once as warm-up and `iterations` times timed.
///
/// # Errors
/// [`BenchError::NothingToMeasure`] when `iterations` or `steps` is zero, or an error of the scene.
pub fn measure_scene3(
    scene: Scene3,
    steps: u32,
    iterations: u32,
    execution: &SolverExecution,
    sleeping: bool,
) -> Result<Measured3, BenchError> {
    if iterations == 0 || steps == 0 {
        return Err(BenchError::NothingToMeasure);
    }
    run_scene3(scene, steps, execution, sleeping)?;
    let mut samples = Vec::new();
    let mut last = None;
    for _ in 0..iterations {
        let mut run = run_scene3(scene, steps, execution, sleeping)?;
        samples.append(&mut run.step_ns);
        last = Some(run);
    }
    let count = samples.len();
    match (summarize(&mut samples), last) {
        (Some(time), Some(run)) => Ok(Measured3 {
            time,
            samples: count,
            run,
        }),
        _ => Err(BenchError::NothingToMeasure),
    }
}

/// The scaling table of a 3D scene (sleeping off, as W1 is timed): the serial scalar reference,
/// the serial lane path, then the pool at every worker count with the scalar and the lane solver.
///
/// # Errors
/// An error of the pool or of the scene; [`BenchError::NothingToMeasure`] for a zero count.
pub fn run_scaling3(
    scene: Scene3,
    steps: u32,
    iterations: u32,
) -> Result<Vec<ScalingRow>, BenchError> {
    scale_with(&|execution| {
        let measured = measure_scene3(scene, steps, iterations, execution, false)?;
        Ok((measured.time, measured.run.hash))
    })
}
