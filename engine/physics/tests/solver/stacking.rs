//! Stacks and the pyramid (the checkpoint scene of card 4-13): everything settles, nothing drifts
//! or sinks beyond a stated bound, energy does not rise, and the result is the same on every run.

use super::common::{DT, crate_of, earth, energy, ground, position, speed, step_n, world};
use base::math::Vec2;
use physics::d2::{BodyDesc, BodyHandle, BodyKind, PhysicsError, Shape2, World};

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

fn pyramid(rows: u32) -> Result<(World, Vec<BodyHandle>), PhysicsError> {
    let mut world = world(128, earth())?;
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
}

fn run_pyramid() -> Result<Run, PhysicsError> {
    let (mut world, boxes) = pyramid(ROWS)?;
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
        if step % 60 == 0 {
            run.hashes.push(world.state_hash());
        }
    }
    Ok(run)
}

#[test]
fn a_ten_high_pyramid_settles_and_stays() -> Result<(), PhysicsError> {
    let run = run_pyramid()?;
    println!(
        "pyramid: settled at step {}, max drift {:.4} m, max penetration {:.4} m, energy {:.3} -> {:.3} (max {:.3})",
        run.settled_at,
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
    Ok(())
}

#[test]
fn the_energy_of_the_pyramid_does_not_rise() -> Result<(), PhysicsError> {
    let run = run_pyramid()?;
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
    Ok(())
}

#[test]
fn the_pyramid_is_the_same_on_every_run() -> Result<(), PhysicsError> {
    let first = run_pyramid()?;
    let second = run_pyramid()?;
    assert_eq!(first.hashes, second.hashes);
    Ok(())
}

/// The state hash of the pyramid after 1, 3 and 5 seconds, measured on Windows and Linux with the
/// feature off and on (card 4-13). A change here is a change of the solver's arithmetic.
const PYRAMID_PINS: [u64; 3] = [
    10_090_729_523_853_206_521,
    11_312_335_679_332_502_761,
    10_058_014_932_819_635_904,
];

#[test]
fn the_pyramid_hashes_to_the_pinned_values() -> Result<(), PhysicsError> {
    let run = run_pyramid()?;
    let seen: Vec<u64> = [0, 2, 4]
        .iter()
        .filter_map(|&i| run.hashes.get(i).copied())
        .collect();
    assert_eq!(seen, PYRAMID_PINS);
    Ok(())
}
