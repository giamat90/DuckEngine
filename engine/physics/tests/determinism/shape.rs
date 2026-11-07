//! The matrix would prove little if its scenes were quiet, or did not have the shape their names
//! promise: a pyramid that does not need several colours, a hub whose contacts all find a colour,
//! sleepers that never sleep, a churn scene that never reuses a slot. Each scene is read from its
//! reference trace and checked against what the name says. The bounds were written before the
//! first measurement (see the review of card 4-18 for any that moved).

use super::recorder::{Trace, record};
use super::scenes::{DROP_STEP, Scene, matrix};
use physics::d2::{MAX_COLOURS, PhysicsError, SolverExecution};

/// Index of the generation in the dump of a body, and of the sleep flag.
const GENERATION: usize = 1;

/// A scene moves if its hash changes in at least this share of the steps (in percent).
const MOVING_PERCENT: usize = 50;

/// What the trace of a scene reached at most.
#[derive(Default, Debug)]
struct Peaks {
    colours: usize,
    overflow: usize,
    solved: usize,
    joints_solved: usize,
    asleep: usize,
    /// Solved contacts and solved joints in the same step.
    both_at_once: bool,
    /// A step in which fewer bodies slept than in the step before.
    woke: bool,
    /// Sleeping bodies just before the ball is dropped.
    asleep_before_drop: usize,
    /// The highest generation of a body at the end: a reused slot.
    generation: u32,
    /// Steps in which the number of live bodies changed.
    population_changes: usize,
    /// Steps in which the hash changed.
    hash_changes: usize,
}

fn peaks(trace: &Trace) -> Peaks {
    let mut peaks = Peaks::default();
    let samples = &trace.samples;
    for (step, sample) in samples.iter().enumerate() {
        let stats = sample.stats;
        peaks.colours = peaks.colours.max(stats.colours);
        peaks.overflow = peaks.overflow.max(stats.overflow);
        peaks.solved = peaks.solved.max(stats.solved);
        peaks.joints_solved = peaks.joints_solved.max(stats.joints_solved);
        peaks.asleep = peaks.asleep.max(sample.asleep);
        peaks.both_at_once |= stats.solved > 0 && stats.joints_solved > 0;
        if step == DROP_STEP as usize - 1 {
            peaks.asleep_before_drop = sample.asleep;
        }
    }
    for pair in samples.windows(2) {
        peaks.woke |= pair[1].asleep < pair[0].asleep;
        peaks.population_changes += usize::from(pair[1].bodies.len() != pair[0].bodies.len());
        peaks.hash_changes += usize::from(pair[1].hash != pair[0].hash);
    }
    peaks.generation = samples
        .last()
        .and_then(|sample| sample.bodies.iter().map(|body| body[GENERATION]).max())
        .unwrap_or(0);
    peaks
}

/// What the name of a scene promises that the trace does not show; `None` when it is kept.
fn broken_promise(name: &str, peaks: &Peaks, steps: usize) -> Option<&'static str> {
    if peaks.hash_changes * 100 < steps * MOVING_PERCENT {
        return Some("the scene is quiet: its hash does not change in half of the steps");
    }
    match name {
        "pyramid" if peaks.colours < 3 => Some("a pyramid needs three colours or more"),
        "row of boxes" if peaks.colours < 2 => Some("a row of boxes needs two colours or more"),
        "chain" if peaks.joints_solved == 0 => Some("a chain solves its hinges"),
        "joints and contacts" if !peaks.both_at_once => {
            Some("hinges and contacts are solved in the same step")
        }
        "colour overflow hub" if peaks.overflow == 0 || peaks.colours < MAX_COLOURS => {
            Some("the hub fills every colour and overflows")
        }
        "sleep and wake" => sleep_promise(peaks),
        "creation and destruction" => churn_promise(peaks),
        n if n.starts_with("scatter") && peaks.solved < 20 => {
            Some("a scatter solves at least twenty contacts at once")
        }
        _ => None,
    }
}

fn sleep_promise(peaks: &Peaks) -> Option<&'static str> {
    if peaks.asleep_before_drop == 0 {
        Some("the pyramids sleep before the ball is dropped")
    } else if !peaks.woke {
        Some("the ball wakes a pyramid")
    } else {
        None
    }
}

fn churn_promise(peaks: &Peaks) -> Option<&'static str> {
    if peaks.population_changes < 3 {
        Some("bodies are created and destroyed between steps")
    } else if peaks.generation == 0 {
        Some("a destroyed slot is reused with a higher generation")
    } else {
        None
    }
}

#[test]
fn the_scenes_have_the_shape_their_names_promise() -> Result<(), PhysicsError> {
    let mut broken = Vec::new();
    for scene in matrix() {
        let trace = record(&scene, &SolverExecution::SERIAL)?;
        let found = peaks(&trace);
        broken.extend(
            broken_promise(scene.name, &found, scene.steps as usize)
                .map(|promise| format!("{}: {promise} ({found:?})", scene.name)),
        );
    }
    assert!(broken.is_empty(), "{}", broken.join("\n"));
    Ok(())
}

/// Steps of the made-up trace of the last test.
const STEPS_CHECKED: usize = 100;

#[test]
fn the_promises_are_checked_by_a_check_that_can_fail() {
    // A scene that moves but does nothing else keeps none of the promises of its name.
    let moving = Peaks {
        hash_changes: STEPS_CHECKED,
        ..Peaks::default()
    };
    let named = |scene: &Scene| broken_promise(scene.name, &moving, STEPS_CHECKED).is_some();
    assert!(matrix().iter().all(named), "every name makes a promise");
    let quiet = Peaks::default();
    assert!(broken_promise("pyramid", &quiet, STEPS_CHECKED).is_some());
}
