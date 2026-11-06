//! The execution matrix: every scene of the catalogue gives the same state hash after every step
//! whether the solver runs the scalar reference serially, the lane path serially, or either of them
//! through a work-stealing pool of 1, 2, 4 or 6 workers (D0 and D1; exit criterion: "replay hash
//! identical across runs and thread counts"). The pattern is the one of `tests/parallel.rs`
//! (card 4-15); the scenes are the stability scenes.

use super::catalogue::{ALL, Build, RESTING, STEPS_TO_SLEEP};
use super::limits::DT;
use super::scene::Scene;
use jobs::pool::WorkStealingPool;
use jobs::scheduler::IScheduler;
use physics::d2::{DEFAULT_CHUNK, PhysicsError, SleepConfig, SolverExecution};
use platform::threads::StdThreads;
use std::sync::Arc;

/// Worker counts of the matrix (owner choice O10: 4 is the scaling target).
const WORKERS: [u32; 4] = [1, 2, 4, 6];

/// A chunk size small enough that the colours of the scenes are cut into several chunks.
const SMALL_CHUNK: u32 = 6;

/// Steps of a scene that stays awake.
const AWAKE_STEPS: u32 = 60;

/// A scene that is awake changes its hash at least this often in `AWAKE_STEPS` steps.
const MIN_CHANGES: usize = 30;

/// Steps of the shape check: enough for the solver to have coloured the contacts.
const SHAPE_STEPS: u32 = 30;

/// The hash and the number of sleeping bodies after one step.
pub type Sample = (u64, usize);

fn pool(workers: u32) -> Result<Arc<dyn IScheduler>, PhysicsError> {
    WorkStealingPool::new(workers, &StdThreads::new())
        .map(|pool| Arc::new(pool) as Arc<dyn IScheduler>)
        .map_err(|_| PhysicsError::NotFinite)
}

/// The execution configurations to compare with the scalar serial reference, by name.
pub fn executions() -> Result<Vec<(String, SolverExecution)>, PhysicsError> {
    let mut all = vec![(
        "lanes, serial".to_owned(),
        SolverExecution {
            scheduler: None,
            simd: true,
            chunk: DEFAULT_CHUNK,
        },
    )];
    for workers in WORKERS {
        let scheduler = pool(workers)?;
        for simd in [false, true] {
            all.push((
                format!("{workers} workers, lanes {simd}"),
                SolverExecution {
                    scheduler: Some(Arc::clone(&scheduler)),
                    simd,
                    chunk: SMALL_CHUNK,
                },
            ));
        }
    }
    Ok(all)
}

/// The hash and sleeping count after each of `steps` steps of the scene `build` makes.
pub fn trace(
    build: &dyn Fn() -> Result<Scene, PhysicsError>,
    execution: SolverExecution,
    steps: u32,
) -> Result<Vec<Sample>, PhysicsError> {
    let mut scene = build()?;
    scene.world.set_execution(execution);
    let mut seen = Vec::new();
    for _ in 0..steps {
        scene.world.step(DT)?;
        seen.push((scene.world.state_hash(), scene.world.sleeping_count()));
    }
    Ok(seen)
}

/// Asserts that every configuration gives the trace of the scalar serial reference.
pub fn assert_same_everywhere(
    name: &str,
    build: &dyn Fn() -> Result<Scene, PhysicsError>,
    steps: u32,
) -> Result<Vec<Sample>, PhysicsError> {
    let reference = trace(build, SolverExecution::SERIAL, steps)?;
    for (label, execution) in executions()? {
        let got = trace(build, execution, steps)?;
        assert_eq!(got, reference, "{name}: {label}");
    }
    Ok(reference)
}

#[test]
fn every_scene_is_the_same_bit_for_bit_in_every_execution_configuration() -> Result<(), PhysicsError>
{
    for entry in &ALL {
        let build = || (entry.build)(SleepConfig::DISABLED);
        let reference = assert_same_everywhere(entry.name, &build, AWAKE_STEPS)?;
        let changes = reference
            .windows(2)
            .filter(|pair| pair[0] != pair[1])
            .count();
        assert!(
            changes >= MIN_CHANGES,
            "{}: the scene moves ({changes} changes)",
            entry.name
        );
    }
    Ok(())
}

fn resting(index: usize) -> Build {
    ALL[index].build
}

#[test]
fn the_scenes_that_fall_asleep_are_the_same_in_every_configuration_including_the_sleep_flags()
-> Result<(), PhysicsError> {
    for (index, steps) in RESTING.into_iter().zip(STEPS_TO_SLEEP) {
        let build = || resting(index)(SleepConfig::DEFAULT);
        let name = ALL[index].name;
        let reference = assert_same_everywhere(name, &build, steps)?;
        let asleep = reference.last().map_or(0, |sample| sample.1);
        assert!(asleep > 0, "{name}: something falls asleep ({asleep})");
    }
    Ok(())
}

#[test]
fn the_matrix_scenes_have_the_shape_their_names_promise() -> Result<(), PhysicsError> {
    for entry in &ALL {
        let mut scene = (entry.build)(SleepConfig::DISABLED)?;
        for _ in 0..SHAPE_STEPS {
            scene.world.step(DT)?;
        }
        let stats = scene.world.solver_stats();
        let joints = scene.world.joint_count();
        assert!(
            stats.solved > 0 || joints > 0,
            "{}: something is solved",
            entry.name
        );
        assert_eq!(
            stats.joints_solved, joints,
            "{}: every joint is solved while awake",
            entry.name
        );
    }
    let mut pyramid = (ALL[0].build)(SleepConfig::DISABLED)?;
    pyramid.world.step(DT)?;
    pyramid.world.step(DT)?;
    assert!(
        pyramid.world.solver_stats().colours > 1,
        "a pyramid needs several colours"
    );
    Ok(())
}
