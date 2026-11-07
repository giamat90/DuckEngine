//! The harness tests itself: a comparison that cannot fail proves nothing (lesson R-23). Here the
//! recorder is shown to see one flipped bit, to name the step, body and field, and to see every
//! deliberately faulty scheduler.

use super::configs::HUGE_CHUNK;
use super::faulty::{Fault, Faulty, LOOP_FAULTS};
use super::recorder::{Trace, record, same};
use super::scenes::{Scene, matrix};
use base::math::Vec2;
use physics::d2::{PhysicsError, SolverExecution, World};
use std::sync::Arc;

/// One constraint per chunk: every colour with two members goes to the scheduler (the hub has
/// colours of one or two members, which a larger chunk keeps on the calling thread).
const CHUNK: u32 = 1;

fn scene(name: &str) -> Result<Scene, PhysicsError> {
    matrix()
        .into_iter()
        .find(|scene| scene.name == name)
        .ok_or(PhysicsError::NotFinite)
}

fn reference(name: &str) -> Result<Trace, PhysicsError> {
    record(&scene(name)?, &SolverExecution::SERIAL)
}

#[test]
fn two_identical_runs_have_no_difference() -> Result<(), PhysicsError> {
    let first = reference("pyramid")?;
    let second = reference("pyramid")?;
    assert_eq!(first.first_difference(&second), None);
    assert_eq!(same(&first, &second, "twice"), Ok(()));
    Ok(())
}

/// The step, body and field of the flipped bit.
const STEP: usize = 5;
const BODY: usize = 2;
const FIELD: usize = 3;

#[test]
fn one_flipped_bit_is_found_and_named() -> Result<(), PhysicsError> {
    let first = reference("pyramid")?;
    let mut second = first.clone();
    second.samples[STEP].bodies[BODY][FIELD] ^= 1;
    let difference = first.first_difference(&second);
    let found = difference.ok_or(PhysicsError::NotFinite)?;
    assert_eq!(found.step, STEP);
    assert!(
        found.what.contains("body 2") && found.what.contains("position.y"),
        "{}",
        found.what
    );
    let message = same(&first, &second, "flipped").err().unwrap_or_default();
    assert!(
        message.contains("flipped") && message.contains("step 5"),
        "{message}"
    );
    Ok(())
}

#[test]
fn a_difference_only_in_the_hash_is_found() -> Result<(), PhysicsError> {
    let first = reference("pyramid")?;
    let mut second = first.clone();
    second.samples[STEP].hash ^= 1;
    let found = first
        .first_difference(&second)
        .ok_or(PhysicsError::NotFinite)?;
    assert_eq!(found.step, STEP);
    assert!(found.what.contains("state hash"), "{}", found.what);
    Ok(())
}

#[test]
fn a_difference_in_the_number_of_bodies_or_steps_is_found() -> Result<(), PhysicsError> {
    let first = reference("pyramid")?;
    let mut fewer_bodies = first.clone();
    fewer_bodies.samples[STEP].bodies.pop();
    let found = first
        .first_difference(&fewer_bodies)
        .ok_or(PhysicsError::NotFinite)?;
    assert!(found.what.contains("live bodies"), "{}", found.what);
    let mut shorter = first.clone();
    shorter.samples.pop();
    let found = first
        .first_difference(&shorter)
        .ok_or(PhysicsError::NotFinite)?;
    assert!(found.what.contains("steps"), "{}", found.what);
    Ok(())
}

/// A copy of the row of boxes in which the first box is thrown one unit in the last place faster.
fn nudged_row() -> Result<Scene, PhysicsError> {
    fn nudge(world: &mut World, step: u32) -> Result<(), PhysicsError> {
        // Slot 0 is the floor; slot 1 is the box that is thrown.
        let thrown = world.iter().nth(1).map(|(handle, _)| handle);
        if let (0, Some(handle)) = (step, thrown) {
            let speed = world.body(handle)?.linear_velocity().x;
            let faster = f32::from_bits(speed.to_bits() + 1);
            world.set_linear_velocity(handle, Vec2::new(faster, 0.0))?;
        }
        Ok(())
    }
    let row = scene("row of boxes")?;
    Ok(Scene {
        events: Some(nudge),
        ..row
    })
}

#[test]
fn a_change_of_one_unit_in_the_last_place_changes_the_trace() -> Result<(), PhysicsError> {
    let first = reference("row of boxes")?;
    let second = record(&nudged_row()?, &SolverExecution::SERIAL)?;
    let found = first
        .first_difference(&second)
        .ok_or(PhysicsError::NotFinite)?;
    assert!(found.step <= 1, "found at step {}", found.step);
    Ok(())
}

/// A chunk that runs twice is not a fault of the solver any more (card 4-30): a chunk builds its
/// result in its own cleared buffer from inputs nobody changes during the pass, so the second run
/// gives the same bits. Before, the impulses of the constraint accumulated twice.
fn seen_by_the_replay(fault: Fault) -> bool {
    !matches!(fault, Fault::RunsFirstChunkTwice)
}

#[test]
fn every_faulty_scheduler_is_seen_on_scenes_whose_colours_go_to_the_scheduler()
-> Result<(), PhysicsError> {
    for name in ["pyramid", "colour overflow hub", "row of boxes"] {
        let expected = reference(name)?;
        for fault in LOOP_FAULTS
            .into_iter()
            .filter(|&fault| seen_by_the_replay(fault))
        {
            let execution = SolverExecution {
                scheduler: Some(Arc::new(Faulty(fault))),
                simd: false,
                chunk: CHUNK,
            };
            let got = record(&scene(name)?, &execution)?;
            let message = same(&expected, &got, "faulty").err();
            assert!(message.is_some(), "{name}: {fault:?} was not seen");
        }
    }
    Ok(())
}

#[test]
fn a_chunk_that_runs_twice_changes_nothing() -> Result<(), PhysicsError> {
    for name in ["pyramid", "colour overflow hub", "row of boxes"] {
        let expected = reference(name)?;
        let execution = SolverExecution {
            scheduler: Some(Arc::new(Faulty(Fault::RunsFirstChunkTwice))),
            simd: false,
            chunk: CHUNK,
        };
        let got = record(&scene(name)?, &execution)?;
        assert_eq!(expected.first_difference(&got), None, "{name}");
    }
    Ok(())
}

#[test]
fn a_faulty_scheduler_that_is_never_asked_changes_nothing() -> Result<(), PhysicsError> {
    // A chunk larger than every colour keeps the loops on the calling thread: the comparison is
    // then equal, which is why a configuration with such a chunk is not counted as using its
    // scheduler.
    let expected = reference("pyramid")?;
    let execution = SolverExecution {
        scheduler: Some(Arc::new(Faulty(Fault::RunsNothing))),
        simd: false,
        chunk: HUGE_CHUNK,
    };
    let got = record(&scene("pyramid")?, &execution)?;
    assert_eq!(expected.first_difference(&got), None);
    Ok(())
}
