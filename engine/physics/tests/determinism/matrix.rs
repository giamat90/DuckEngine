//! The matrix: every scene under every configuration, each run `RUNS` times, compared bit for bit
//! with the scalar serial reference (D0: the same run again; D1: another worker count, scheduler,
//! order, grouping, lane path or chunk size). Nothing is stored. The same run also checks that the
//! configurations did what their names say, so that a comparison that never left the calling
//! thread cannot pass for D1.

use super::configs::{Config, ODD_CHUNK, configs};
use super::recorder::{Trace, record, same};
use super::scenes::{Scene, long_runs, matrix};
use physics::d2::{PhysicsError, SolverExecution};

/// Scenes whose colours have several chunks at `ODD_CHUNK`, so every configuration that uses its
/// scheduler must have been asked at least once in each of them.
const BUSY_SCENES: [&str; 5] = [
    "pyramid",
    "row of boxes",
    "scatter seed 3",
    "scatter seed 11",
    "joints and contacts",
];

/// The configurations that run the long scenes: the lanes, a pool, an order that is not the
/// natural one, and a shuffle.
const LONG_CONFIGS: [&str; 4] = [
    "serial, lanes",
    "pool of 3, chunk 7, lanes",
    "reversed, chunk 1, scalar",
    "shuffled 0x63, chunk 7, lanes",
];

/// What a configuration was asked in each scene: `(scene, calls to the scheduler)`.
type Usage = Vec<(&'static str, usize)>;

/// The traces of the scalar serial reference, twice (D0 of the reference itself).
fn references(scenes: &[Scene], problems: &mut Vec<String>) -> Result<Vec<Trace>, PhysicsError> {
    let mut all = Vec::new();
    for scene in scenes {
        let first = record(scene, &SolverExecution::SERIAL)?;
        let again = record(scene, &SolverExecution::SERIAL)?;
        let context = format!("{} / serial reference / run 2", scene.name);
        problems.extend(same(&first, &again, &context).err());
        all.push(first);
    }
    Ok(all)
}

/// Runs `scene` under `config` `RUNS` times; differences go to `problems`. Returns how many calls
/// the scheduler of the configuration got in the first run.
fn run_scene(
    config: &Config,
    scene: &Scene,
    reference: &Trace,
    problems: &mut Vec<String>,
) -> Result<usize, PhysicsError> {
    let calls = || config.probe.as_ref().map_or(0, |probe| probe.calls());
    let before = calls();
    let mut asked = 0;
    for run in 0..config.runs {
        let got = record(scene, &config.execution)?;
        let context = format!("{} / {} / run {}", scene.name, config.name, run + 1);
        problems.extend(same(reference, &got, &context).err());
        if run == 0 {
            asked = calls() - before;
        }
    }
    Ok(asked)
}

/// Runs `config` on every scene; returns what its scheduler was asked in each.
fn run_config(
    config: &Config,
    scenes: &[Scene],
    references: &[Trace],
    problems: &mut Vec<String>,
) -> Result<Usage, PhysicsError> {
    let mut usage = Vec::new();
    for (scene, reference) in scenes.iter().zip(references) {
        let asked = run_scene(config, scene, reference, problems)?;
        usage.push((scene.name, asked));
    }
    Ok(usage)
}

/// The ways a configuration quietly did not do what it says: it has a scheduler it is meant to
/// use, and the scheduler was never asked in a scene that has colours with several chunks.
fn idle_schedulers(config: &Config, usage: &Usage) -> Vec<String> {
    if !config.uses_scheduler || config.execution.chunk > ODD_CHUNK {
        return Vec::new();
    }
    usage
        .iter()
        .filter(|(scene, calls)| *calls == 0 && BUSY_SCENES.contains(scene))
        .map(|(scene, _)| format!("{}: the scheduler was never asked in {scene}", config.name))
        .collect()
}

#[test]
fn every_scene_is_the_same_in_every_configuration_and_every_scheduler_is_used()
-> Result<(), PhysicsError> {
    let scenes = matrix();
    let mut problems = Vec::new();
    let references = references(&scenes, &mut problems)?;
    let mut idle = Vec::new();
    for config in configs()? {
        let usage = run_config(&config, &scenes, &references, &mut problems)?;
        idle.extend(idle_schedulers(&config, &usage));
    }
    assert!(
        problems.is_empty(),
        "{} runs differ from the reference:\n{}",
        problems.len(),
        problems.join("\n")
    );
    assert!(idle.is_empty(), "silent serial runs:\n{}", idle.join("\n"));
    Ok(())
}

/// The long scenes: 600 steps, so a slow drift (a tiny difference that grows) shows.
#[test]
fn the_long_runs_stay_identical_for_ten_seconds_of_simulation() -> Result<(), PhysicsError> {
    let scenes = long_runs();
    let mut problems = Vec::new();
    let references = references(&scenes, &mut problems)?;
    let mut ran = 0;
    for config in configs()? {
        if LONG_CONFIGS.contains(&config.name.as_str()) {
            run_config(&config, &scenes, &references, &mut problems)?;
            ran += 1;
        }
    }
    assert_eq!(ran, LONG_CONFIGS.len(), "every long configuration exists");
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    Ok(())
}

/// The pool of the matrix that has the most workers must have run chunks on more than one thread
/// over the scenes, or the comparison with the reference would be a comparison of serial runs.
const BIGGEST_POOL: &str = "pool of 7, chunk 7, scalar";

#[test]
fn a_pool_of_seven_runs_chunks_on_more_than_one_thread() -> Result<(), PhysicsError> {
    let scenes = matrix();
    let config = configs()?
        .into_iter()
        .find(|config| config.name == BIGGEST_POOL)
        .ok_or(PhysicsError::NotFinite)?;
    for scene in scenes
        .iter()
        .filter(|scene| BUSY_SCENES.contains(&scene.name))
    {
        record(scene, &config.execution)?;
    }
    let threads = config.probe.as_ref().map_or(0, |probe| probe.threads());
    assert!(threads >= 2, "chunks ran on {threads} thread(s)");
    Ok(())
}
