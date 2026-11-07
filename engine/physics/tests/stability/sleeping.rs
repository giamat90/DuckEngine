//! Sleeping variants of the settled scenes: each falls asleep by a stated step, stays where it was
//! within the drift bound, and while it is asleep the state is bit for bit the same step after
//! step. Until the first body falls asleep the world with sleeping on is bit-identical to the
//! world with sleeping off (card 4-14).

use super::catalogue::{ALL, RESTING};
use super::limits::{DRIFT, DT};
use super::measure::{below, dynamic_count, positions, record};
use super::scene::Scene;
use physics::d2::{PhysicsError, SleepConfig};

/// Every resting scene must be asleep by this step (5 s): the pyramid of 1 m boxes of card 4-14
/// was asleep at step 36.
const ASLEEP_BY: u32 = 300;

/// The mixed scene has circles in a bin, which settle more slowly: asleep by this step (10 s).
const MIXED_ASLEEP_BY: u32 = 600;

/// Steps over which an asleep scene must not change a bit.
const HOLD_STEPS: u32 = 120;

/// The least number of steps with sleeping on and off that must be compared before the first
/// sleeper, so that the equality is a real stretch of work and not one step.
const MIN_EQUAL_STEPS: u32 = 20;

/// The step by which `ALL[index]` must be asleep.
fn deadline(index: usize) -> u32 {
    if ALL[index].name.starts_with("mixed") {
        MIXED_ASLEEP_BY
    } else {
        ASLEEP_BY
    }
}

/// Steps `scene` until every dynamic body sleeps or `deadline` passes; the step it happened at.
fn fall_asleep(scene: &mut Scene, deadline: u32) -> Result<Option<u32>, PhysicsError> {
    let dynamic = dynamic_count(&scene.world);
    for step in 1..=deadline {
        scene.world.step(DT)?;
        if scene.world.sleeping_count() == dynamic {
            return Ok(Some(step));
        }
    }
    Ok(None)
}

#[test]
fn every_resting_scene_falls_asleep_by_its_step_and_stays_bit_identical_while_asleep()
-> Result<(), PhysicsError> {
    for index in RESTING {
        let name = format!("{} asleep", ALL[index].name);
        let mut scene = (ALL[index].build)(SleepConfig::DEFAULT)?;
        assert_eq!(scene.world.sleeping_count(), 0, "{name}: it starts awake");
        let start = positions(&scene)?;
        let found = fall_asleep(&mut scene, deadline(index))?;
        assert!(
            found.is_some(),
            "{name}: not asleep by step {}",
            deadline(index)
        );
        let at = found.unwrap_or(0);
        record(
            &name,
            "asleep at step",
            f64::from(at),
            f64::from(deadline(index)),
        );
        // The hash changes once more on the step after the last island falls asleep (the warm
        // start of that step is stored one step late) and is constant from then on.
        scene.world.step(DT)?;
        let hash = scene.world.state_hash();
        let asleep_at = positions(&scene)?;
        for _ in 0..HOLD_STEPS {
            scene.world.step(DT)?;
            assert_eq!(
                scene.world.state_hash(),
                hash,
                "{name}: a bit changed while asleep"
            );
        }
        assert_eq!(positions(&scene)?, asleep_at, "{name}: a position changed");
        let drift = asleep_at
            .iter()
            .zip(&start)
            .map(|(now, from)| (*now - *from).length())
            .fold(0.0, f32::max);
        below(&name, "drift at sleep (m)", drift, DRIFT);
    }
    Ok(())
}

/// Steps the scene `index` with sleeping off and on until the sleeping one first has a sleeper,
/// asserting equal hashes on the way; the number of equal steps, or `None` when nothing fell asleep
/// in `ASLEEP_BY` steps.
fn equal_until_sleep(index: usize) -> Result<Option<u32>, PhysicsError> {
    let name = ALL[index].name;
    let mut awake = (ALL[index].build)(SleepConfig::DISABLED)?;
    let mut sleepy = (ALL[index].build)(SleepConfig::DEFAULT)?;
    for equal_steps in 0..ASLEEP_BY {
        awake.world.step(DT)?;
        sleepy.world.step(DT)?;
        if sleepy.world.sleeping_count() > 0 {
            return Ok(Some(equal_steps));
        }
        assert_eq!(
            awake.world.state_hash(),
            sleepy.world.state_hash(),
            "{name}"
        );
    }
    Ok(None)
}

#[test]
fn with_sleeping_on_a_scene_equals_the_scene_with_it_off_until_the_first_body_sleeps()
-> Result<(), PhysicsError> {
    for index in RESTING {
        let equal_steps = equal_until_sleep(index)?;
        assert!(
            equal_steps.is_some_and(|steps| steps >= MIN_EQUAL_STEPS),
            "{}: {equal_steps:?} equal steps is not a real stretch",
            ALL[index].name
        );
    }
    Ok(())
}
