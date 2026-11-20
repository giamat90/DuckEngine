//! Card 8-49: how much room the given spin leaves, measured on boxes. A box that spins and moves
//! fast hits another box; the corner of a spinning box moves faster than its centre, so a contact
//! can throw a speed past the bound. The refusals depend on speed and spin together: at the
//! largest given speed they begin at a spin of a few hundred radians per second, at a fifth of that
//! speed only near the largest given spin, and nothing was refused at a spin of 250 or less.
//! Observations of the solver in two scenes, in 2D and 3D, pinned at a few points (not promises).

use base::math::{Vec2, Vec3};
use physics::core::PhysicsError;
use physics::{d2, d3};

const DT: f32 = 1.0 / 60.0;
const STEPS: u32 = 30;
/// Speeds of the grid: from a fast bullet up to the largest given speed.
const SPEEDS: [f32; 4] = [1000.0, 2500.0, 3500.0, 5000.0];
const MASS_RATIOS: [f32; 2] = [10.0, 1000.0];

/// `Thrown`: a heavy box that spins and moves into a light one at rest. `HeadOn`: a light box and
/// a heavy one go at each other and spin the opposite ways (restitution a half).
#[derive(Clone, Copy)]
enum Scene {
    Thrown,
    HeadOn,
}

/// What each of the two bodies of a scene starts with: x, speed, spin, density.
type Start = [(f32, f32, f32, f32); 2];

fn starts(scene: Scene, ratio: f32, speed: f32, spin: f32) -> Start {
    match scene {
        Scene::Thrown => [(0.0, speed, spin, ratio), (0.9, 0.0, 0.0, 1.0)],
        Scene::HeadOn => [(-0.45, speed, spin, 1.0), (0.45, -speed, -spin, ratio)],
    }
}

fn restitution(scene: Scene) -> f32 {
    match scene {
        Scene::Thrown => 1.0,
        Scene::HeadOn => 0.5,
    }
}

fn steps2(start: Start, restitution: f32) -> Result<bool, PhysicsError> {
    let mut world = d2::World::new(d2::WorldConfig {
        max_bodies: 4,
        gravity: Vec2::ZERO,
        max_pairs: 16,
        broadphase_margin: d2::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 16,
        max_joints: 0,
        speculative_margin: d2::DEFAULT_SPECULATIVE_MARGIN,
        solver: d2::SolverConfig::DEFAULT,
        sleep: d2::SleepConfig::DISABLED,
    })?;
    for (x, speed, spin, density) in start {
        world.create_body(&d2::BodyDesc {
            linear_velocity: Vec2::new(speed, 0.0),
            angular_velocity: spin,
            density,
            restitution,
            ..d2::BodyDesc::new(
                d2::BodyKind::Dynamic,
                Vec2::new(x, 0.0),
                d2::Shape2::Box {
                    half_extents: Vec2::new(0.5, 0.5),
                },
            )
        })?;
    }
    Ok((0..STEPS).all(|_| world.step(DT).is_ok()))
}

fn steps3(start: Start, restitution: f32) -> Result<bool, PhysicsError> {
    let mut world = d3::World::new(d3::WorldConfig {
        max_bodies: 4,
        gravity: Vec3::ZERO,
        max_pairs: 16,
        broadphase_margin: d3::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 16,
        max_joints: 0,
        speculative_margin: d3::DEFAULT_SPECULATIVE_MARGIN,
        solver: d3::SolverConfig::DEFAULT,
        sleep: d3::SleepConfig::DISABLED,
    })?;
    for (x, speed, spin, density) in start {
        world.create_body(&d3::BodyDesc {
            linear_velocity: Vec3::new(speed, 0.0, 0.0),
            angular_velocity: Vec3::new(0.0, 0.0, spin),
            density,
            restitution,
            ..d3::BodyDesc::new(
                d3::BodyKind::Dynamic,
                Vec3::new(x, 0.0, 0.0),
                d3::Shape3::Box {
                    half_extents: Vec3::new(0.5, 0.5, 0.5),
                },
            )
        })?;
    }
    Ok((0..STEPS).all(|_| world.step(DT).is_ok()))
}

/// Whether the scene steps `STEPS` times in 2D and in 3D.
fn steps(scene: Scene, ratio: f32, speed: f32, spin: f32) -> Result<[bool; 2], PhysicsError> {
    let (start, restitution) = (starts(scene, ratio, speed, spin), restitution(scene));
    Ok([steps2(start, restitution)?, steps3(start, restitution)?])
}

/// The outcomes of every scene, ratio and dimension at `speed` and `spin`.
fn grid(speed: f32, spin: f32) -> Result<Vec<bool>, PhysicsError> {
    let mut outcomes = Vec::new();
    for scene in [Scene::Thrown, Scene::HeadOn] {
        for ratio in MASS_RATIOS {
            outcomes.extend(steps(scene, ratio, speed, spin)?);
        }
    }
    Ok(outcomes)
}

#[test]
fn nothing_is_refused_at_a_spin_of_250_or_less_at_any_given_speed() -> Result<(), PhysicsError> {
    for speed in SPEEDS {
        assert!(grid(speed, 250.0)?.iter().all(|&ok| ok), "speed {speed}");
    }
    Ok(())
}

#[test]
fn nothing_is_refused_at_650_rad_per_second_up_to_1000_metres_per_second()
-> Result<(), PhysicsError> {
    assert!(grid(1000.0, 650.0)?.iter().all(|&ok| ok));
    Ok(())
}

#[test]
fn every_scene_is_refused_at_the_largest_given_speed_with_a_spin_of_600() -> Result<(), PhysicsError>
{
    assert!(grid(d2::MAX_GIVEN_SPEED, 600.0)?.iter().all(|&ok| !ok));
    Ok(())
}

#[test]
fn no_scene_is_refused_at_200_metres_per_second_or_less_even_at_the_largest_given_spin()
-> Result<(), PhysicsError> {
    assert!(grid(200.0, d2::MAX_GIVEN_SPIN)?.iter().all(|&ok| ok));
    Ok(())
}

#[test]
fn the_largest_given_spin_is_refused_in_most_scenes_at_2500_metres_per_second()
-> Result<(), PhysicsError> {
    let outcomes = grid(2500.0, d2::MAX_GIVEN_SPIN)?;
    // Seven of the eight were refused when measured; the eighth (a light head-on box against a
    // box ten times heavier, in 3D) steps.
    assert!(
        outcomes.iter().filter(|&&ok| !ok).count() >= 6,
        "{outcomes:?}"
    );
    Ok(())
}
