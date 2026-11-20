//! Card 8-47: what the speed headroom of card 8-46 does not cover, pinned.
//!
//! `create_body` and the setters accept at most `MAX_GIVEN_SPEED`, half of the bound a step
//! enforces. That room covers gravity, damping and forces. A collision between a light body and a
//! much heavier one, both moving at the given limit, can send the light body out at up to three
//! times its speed (an elastic bounce off a body that barely moves), above the bound, and the step
//! refuses it as `Diverged`. The owner chose to keep the bound as a ceiling and document this
//! (2026-11-20); whether a chain of such collisions could pass any fixed headroom was not tried (card 8-48). These tests pin the
//! behaviour in 2D and 3D: the refusal, that it leaves the world untouched, that the same
//! collision at a lower speed is fine, and that the way out works (set the speed down).

use base::math::{Vec2, Vec3};
use physics::core::PhysicsError;
use physics::{d2, d3};

const DT: f32 = 1.0 / 60.0;
/// Two bodies of radius 0.5 whose centres are 0.9 apart overlap a little, so the contact exists
/// in the first step (at these speeds a body travels 80 m a step and would skip a contact that
/// starts further away: the engine has no continuous collision detection).
const HALF_GAP: f32 = 0.45;
/// The mass ratios under test: from "ten times" (already refused) to an absurd million.
const HEAVY_DENSITIES: [f32; 3] = [10.0, 1.0e3, 1.0e6];

fn world2() -> Result<d2::World, PhysicsError> {
    d2::World::new(d2::WorldConfig {
        max_bodies: 4,
        gravity: Vec2::ZERO,
        max_pairs: 16,
        broadphase_margin: d2::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 16,
        max_joints: 0,
        speculative_margin: d2::DEFAULT_SPECULATIVE_MARGIN,
        solver: d2::SolverConfig::DEFAULT,
        sleep: d2::SleepConfig::DISABLED,
    })
}

fn world3() -> Result<d3::World, PhysicsError> {
    d3::World::new(d3::WorldConfig {
        max_bodies: 4,
        gravity: Vec3::ZERO,
        max_pairs: 16,
        broadphase_margin: d3::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 16,
        max_joints: 0,
        speculative_margin: d3::DEFAULT_SPECULATIVE_MARGIN,
        solver: d3::SolverConfig::DEFAULT,
        sleep: d3::SleepConfig::DISABLED,
    })
}

/// A light elastic ball going right and a heavy one going left, both at `speed`, touching.
fn head_on2(
    speed: f32,
    heavy: f32,
) -> Result<(d2::World, d2::BodyHandle, d2::BodyHandle), PhysicsError> {
    let mut world = world2()?;
    let mut ball = |x: f32, velocity: f32, density: f32| {
        world.create_body(&d2::BodyDesc {
            linear_velocity: Vec2::new(velocity, 0.0),
            density,
            restitution: 1.0,
            ..d2::BodyDesc::new(
                d2::BodyKind::Dynamic,
                Vec2::new(x, 0.0),
                d2::Shape2::Circle { radius: 0.5 },
            )
        })
    };
    let light = ball(-HALF_GAP, speed, 1.0)?;
    let heavy = ball(HALF_GAP, -speed, heavy)?;
    Ok((world, light, heavy))
}

fn head_on3(
    speed: f32,
    heavy: f32,
) -> Result<(d3::World, d3::BodyHandle, d3::BodyHandle), PhysicsError> {
    let mut world = world3()?;
    let mut ball = |x: f32, velocity: f32, density: f32| {
        world.create_body(&d3::BodyDesc {
            linear_velocity: Vec3::new(velocity, 0.0, 0.0),
            density,
            restitution: 1.0,
            ..d3::BodyDesc::new(
                d3::BodyKind::Dynamic,
                Vec3::new(x, 0.0, 0.0),
                d3::Shape3::Sphere { radius: 0.5 },
            )
        })
    };
    let light = ball(-HALF_GAP, speed, 1.0)?;
    let heavy = ball(HALF_GAP, -speed, heavy)?;
    Ok((world, light, heavy))
}

/// The light body of a head-on hit with a body a thousand times heavier leaves at almost, and
/// not above, three times its speed (measured 2.996): the elastic bounce off a wall that moves.
fn leaves_at_almost_three_times(speed_after: f32, speed_before: f32) -> bool {
    let ratio = speed_after.abs() / speed_before;
    ratio > 2.9 && ratio < 3.0
}

#[test]
fn two_bodies_given_the_top_speed_and_unequal_masses_are_refused_as_diverged_in_3d()
-> Result<(), PhysicsError> {
    for heavy in HEAVY_DENSITIES {
        let (mut world, _, _) = head_on3(d3::MAX_GIVEN_SPEED, heavy)?;
        let (hash, steps) = (world.state_hash(), world.step_count());
        assert_eq!(
            world.step(DT),
            Err(PhysicsError::Diverged),
            "heavy x{heavy}"
        );
        assert_eq!(
            (world.state_hash(), world.step_count()),
            (hash, steps),
            "heavy x{heavy}"
        );
    }
    Ok(())
}

#[test]
fn two_bodies_given_the_top_speed_and_unequal_masses_are_refused_as_diverged_in_2d()
-> Result<(), PhysicsError> {
    for heavy in HEAVY_DENSITIES {
        let (mut world, _, _) = head_on2(d2::MAX_GIVEN_SPEED, heavy)?;
        let (hash, steps) = (world.state_hash(), world.step_count());
        assert_eq!(
            world.step(DT),
            Err(PhysicsError::Diverged),
            "heavy x{heavy}"
        );
        assert_eq!(
            (world.state_hash(), world.step_count()),
            (hash, steps),
            "heavy x{heavy}"
        );
    }
    Ok(())
}

#[test]
fn equal_masses_at_the_top_speed_are_not_refused() -> Result<(), PhysicsError> {
    let (mut world, _, _) = head_on3(d3::MAX_GIVEN_SPEED, 1.0)?;
    assert_eq!(world.step(DT), Ok(()));
    let (mut world, _, _) = head_on2(d2::MAX_GIVEN_SPEED, 1.0)?;
    assert_eq!(world.step(DT), Ok(()));
    Ok(())
}

#[test]
fn the_same_collision_at_a_lower_speed_steps_and_the_light_body_leaves_faster_than_it_came()
-> Result<(), PhysicsError> {
    let speed = 1000.0;
    let (mut world, light, _) = head_on3(speed, 1.0e3)?;
    assert_eq!(world.step(DT), Ok(()));
    let after = world.body(light)?.linear_velocity().x;
    assert!(leaves_at_almost_three_times(after, speed), "3D: {after}");
    let (mut world, light, _) = head_on2(speed, 1.0e3)?;
    assert_eq!(world.step(DT), Ok(()));
    let after = world.body(light)?.linear_velocity().x;
    assert!(leaves_at_almost_three_times(after, speed), "2D: {after}");
    Ok(())
}

#[test]
fn a_refused_collision_steps_again_once_a_speed_is_set_down() -> Result<(), PhysicsError> {
    let (mut world, light, heavy) = head_on3(d3::MAX_GIVEN_SPEED, 1.0e3)?;
    assert_eq!(world.step(DT), Err(PhysicsError::Diverged));
    world.set_linear_velocity(light, Vec3::new(1000.0, 0.0, 0.0))?;
    world.set_linear_velocity(heavy, Vec3::new(-1000.0, 0.0, 0.0))?;
    assert_eq!(world.step(DT), Ok(()));
    let (mut world, light, heavy) = head_on2(d2::MAX_GIVEN_SPEED, 1.0e3)?;
    assert_eq!(world.step(DT), Err(PhysicsError::Diverged));
    world.set_linear_velocity(light, Vec2::new(1000.0, 0.0))?;
    world.set_linear_velocity(heavy, Vec2::new(-1000.0, 0.0))?;
    assert_eq!(world.step(DT), Ok(()));
    Ok(())
}
