//! Stacks and the pyramid (the checkpoint scene of card 4-13): everything settles, nothing drifts
//! or sinks beyond a stated bound, energy does not rise, and the result is the same on every run.

use super::common::{
    DT, crate_of, earth, energy, ground, position, speed, step_n, world, world_sleeping,
};
use base::math::Vec2;
use physics::d2::{BodyDesc, BodyHandle, BodyKind, PhysicsError, Shape2, SleepConfig, World};

const HALF: Vec2 = Vec2::new(0.5, 0.5);
/// Rows of the pyramid: 10 high is 55 boxes.
const ROWS: u32 = 10;
/// Seconds simulated for the pyramid.
const PYRAMID_SECONDS: u32 = 5;
/// A body is at rest below this speed in metres per second.
const REST_SPEED: f32 = 0.02;
/// No box may end further than this from where it started: the "0.05 m" of the reference workload.
const MAX_DRIFT: f32 = 0.05;
/// No contact may be deeper than this.
const MAX_PENETRATION: f32 = 0.02;

fn pyramid(rows: u32, sleep: SleepConfig) -> Result<(World, Vec<BodyHandle>), PhysicsError> {
    let mut world = world_sleeping(128, earth(), sleep)?;
    ground(&mut world)?;
    let mut boxes = Vec::new();
    for row in 0..rows {
        let count = rows - row;
        for i in 0..count {
            let x = (f32::from(u8::try_from(i).unwrap_or(0))
                - f32::from(u8::try_from(count - 1).unwrap_or(0)) / 2.0)
                * 1.0;
            let y = 0.5 + f32::from(u8::try_from(row).unwrap_or(0));
            boxes.push(crate_of(&mut world, HALF, x, y)?);
        }
    }
    Ok((world, boxes))
}

/// The deepest overlap among the current manifolds, as a positive number (zero when none overlap).
fn deepest(world: &World) -> f32 {
    world
        .manifolds()
        .as_slice()
        .iter()
        .flat_map(|entry| entry.manifold.points())
        .map(|point| -point.separation)
        .fold(0.0, f32::max)
}

fn fastest(world: &World, boxes: &[BodyHandle]) -> Result<f32, PhysicsError> {
    let mut worst = 0.0_f32;
    for &b in boxes {
        worst = worst.max(speed(world, b)?);
    }
    Ok(worst)
}

fn farthest(world: &World, boxes: &[BodyHandle], start: &[Vec2]) -> Result<f32, PhysicsError> {
    let mut worst = 0.0_f32;
    for (&b, &from) in boxes.iter().zip(start) {
        worst = worst.max((position(world, b)? - from).length());
    }
    Ok(worst)
}

fn starts(world: &World, boxes: &[BodyHandle]) -> Result<Vec<Vec2>, PhysicsError> {
    boxes.iter().map(|&b| position(world, b)).collect()
}

#[test]
fn stacks_of_two_to_five_boxes_stay_where_they_are() -> Result<(), PhysicsError> {
    for height in 2..=5_u8 {
        let mut world = world(16, earth())?;
        ground(&mut world)?;
        let mut boxes = Vec::new();
        for level in 0..height {
            boxes.push(crate_of(&mut world, HALF, 0.0, 0.5 + f32::from(level))?);
        }
        let start = starts(&world, &boxes)?;
        step_n(&mut world, 300)?;
        let drift = farthest(&world, &boxes, &start)?;
        assert!(drift < 0.03, "height {height}: drift {drift}");
        assert!(fastest(&world, &boxes)? < REST_SPEED, "height {height}");
        assert!(deepest(&world) < MAX_PENETRATION, "height {height}");
    }
    Ok(())
}

/// A box of the given density (kilograms per square metre).
fn heavy(world: &mut World, density: f32, y: f32) -> Result<BodyHandle, PhysicsError> {
    let mut desc = BodyDesc::new(
        BodyKind::Dynamic,
        Vec2::new(0.0, y),
        Shape2::Box { half_extents: HALF },
    );
    desc.density = density;
    world.create_body(&desc)
}

#[test]
fn a_box_ten_times_heavier_than_the_one_under_it_does_not_crush_or_throw_it()
-> Result<(), PhysicsError> {
    for (low, high) in [(1.0, 10.0), (10.0, 1.0)] {
        let mut world = world(16, earth())?;
        ground(&mut world)?;
        let bottom = heavy(&mut world, low, 0.5)?;
        let top = heavy(&mut world, high, 1.5)?;
        let boxes = [bottom, top];
        let start = starts(&world, &boxes)?;
        step_n(&mut world, 300)?;
        let drift = farthest(&world, &boxes, &start)?;
        assert!(drift < 0.05, "densities {low}/{high}: drift {drift}");
        assert!(fastest(&world, &boxes)? < REST_SPEED);
        assert!(deepest(&world) < MAX_PENETRATION, "{}", deepest(&world));
    }
    Ok(())
}

/// What the pyramid run measured.
struct Run {
    /// The first step after which every box stays below `REST_SPEED` until the end.
    settled_at: u32,
    max_drift: f32,
    max_penetration: f32,
    first_energy: f64,
    max_energy: f64,
    last_energy: f64,
    hashes: Vec<u64>,
    /// The first step after which every box sleeps (card 4-14); `None` when sleeping is off.
    asleep_at: Option<u32>,
}

fn run_pyramid(sleep: SleepConfig) -> Result<Run, PhysicsError> {
    let (mut world, boxes) = pyramid(ROWS, sleep)?;
    let start = starts(&world, &boxes)?;
    let g = earth().y;
    let first_energy = energy(&world, g);
    let mut run = Run {
        settled_at: 0,
        max_drift: 0.0,
        max_penetration: 0.0,
        first_energy,
        max_energy: first_energy,
        last_energy: first_energy,
        hashes: Vec::new(),
        asleep_at: None,
    };
    for step in 1..=PYRAMID_SECONDS * 60 {
        world.step(DT)?;
        if fastest(&world, &boxes)? >= REST_SPEED {
            run.settled_at = step;
        }
        run.max_drift = run.max_drift.max(farthest(&world, &boxes, &start)?);
        run.max_penetration = run.max_penetration.max(deepest(&world));
        run.last_energy = energy(&world, g);
        run.max_energy = run.max_energy.max(run.last_energy);
        if run.asleep_at.is_none() && world.sleeping_count() == boxes.len() {
            run.asleep_at = Some(step);
        }
        if step % 60 == 0 {
            run.hashes.push(world.state_hash());
        }
    }
    Ok(run)
}

/// Both configurations: sleeping must not be what makes the pyramid pass the bounds of card 4-13.
const CONFIGS: [SleepConfig; 2] = [SleepConfig::DISABLED, SleepConfig::DEFAULT];

#[test]
fn a_ten_high_pyramid_settles_and_stays() -> Result<(), PhysicsError> {
    for sleep in CONFIGS {
        let run = run_pyramid(sleep)?;
        println!(
            "pyramid (sleeping {}): settled at step {}, asleep at {:?}, max drift {:.4} m, max penetration {:.4} m, energy {:.3} -> {:.3} (max {:.3})",
            sleep.enabled,
            run.settled_at,
            run.asleep_at,
            run.max_drift,
            run.max_penetration,
            run.first_energy,
            run.last_energy,
            run.max_energy
        );
        assert!(
            run.settled_at < (PYRAMID_SECONDS - 1) * 60,
            "settled at {}",
            run.settled_at
        );
        assert!(run.max_drift < MAX_DRIFT, "drift {}", run.max_drift);
        assert!(
            run.max_penetration < MAX_PENETRATION,
            "penetration {}",
            run.max_penetration
        );
    }
    Ok(())
}

#[test]
fn the_energy_of_the_pyramid_does_not_rise() -> Result<(), PhysicsError> {
    for sleep in CONFIGS {
        let run = run_pyramid(sleep)?;
        assert!(
            run.max_energy <= run.first_energy + 0.5,
            "{} -> max {}",
            run.first_energy,
            run.max_energy
        );
        assert!(
            run.last_energy < run.first_energy,
            "{} -> {}",
            run.first_energy,
            run.last_energy
        );
    }
    Ok(())
}

#[test]
fn the_pyramid_is_the_same_on_every_run() -> Result<(), PhysicsError> {
    for sleep in CONFIGS {
        let first = run_pyramid(sleep)?;
        let second = run_pyramid(sleep)?;
        assert_eq!(first.hashes, second.hashes);
        assert_eq!(first.asleep_at, second.asleep_at);
    }
    Ok(())
}

#[test]
fn the_pyramid_sleeps_only_when_sleeping_is_on() -> Result<(), PhysicsError> {
    assert_eq!(run_pyramid(SleepConfig::DISABLED)?.asleep_at, None);
    let asleep = run_pyramid(SleepConfig::DEFAULT)?.asleep_at;
    assert_eq!(asleep, Some(PYRAMID_ASLEEP_AT));
    Ok(())
}

/// The step after which all 55 boxes of the pyramid sleep: a count of steps, the same on every
/// machine. Every box of the pyramid moves at 0.05 m/s or less from about step 6 on, so the island sleeps
/// 30 steps (the time to sleep) later, while the stack is still sinking by a few hundredths of a
/// millimetre per step; the bounds of `a_ten_high_pyramid_settles_and_stays` hold either way.
const PYRAMID_ASLEEP_AT: u32 = 36;

/// The state hash of the pyramid after 1, 3 and 5 seconds with sleeping off, measured on Windows and
/// Linux with the feature off and on. Card 4-13 measured `10_090_729_523_853_206_521`,
/// `11_312_335_679_332_502_761` and `10_058_014_932_819_635_904`; card 4-14 re-pinned them on
/// purpose because the constraints are solved colour by colour instead of in manifold-list order,
/// which gives other bits (the pyramid still passes every bound of card 4-13).
const PYRAMID_PINS: [u64; 3] = [
    11_531_489_991_407_575_028,
    10_829_217_195_096_048_855,
    7_972_265_619_179_479_880,
];

/// The same with sleeping on (the default): the whole pyramid sleeps from `PYRAMID_ASLEEP_AT` on,
/// so the three values are one, and differ from the sleeping-off ones in the sleep flag of each box.
const PYRAMID_SLEEPING_PINS: [u64; 3] = [12_752_703_100_913_107_270; 3];

fn pinned(sleep: SleepConfig) -> Result<Vec<u64>, PhysicsError> {
    let run = run_pyramid(sleep)?;
    Ok([0, 2, 4]
        .iter()
        .filter_map(|&i| run.hashes.get(i).copied())
        .collect())
}

#[test]
fn the_pyramid_hashes_to_the_pinned_values() -> Result<(), PhysicsError> {
    assert_eq!(pinned(SleepConfig::DISABLED)?, PYRAMID_PINS);
    assert_eq!(pinned(SleepConfig::DEFAULT)?, PYRAMID_SLEEPING_PINS);
    Ok(())
}
