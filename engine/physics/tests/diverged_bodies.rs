//! Card 8-49: which bodies crossed the bound in a refused step. `PhysicsError::Diverged` names no
//! body, and in the refused chain of card 8-48 destroying the fast first body did not cure it;
//! `World::diverged_bodies` names the bodies whose speed passed the bound, in slot order. Card
//! 8-38: it names the bodies of a step refused as `NotFinite` too.

use base::math::{Vec2, Vec3};
use physics::core::PhysicsError;
use physics::{d2, d3};

const DT: f32 = 1.0 / 60.0;
/// Overlapping neighbours of half side 0.5, so the contact exists in the first step.
const SPACING: f32 = 0.9;

fn world2() -> Result<d2::World, PhysicsError> {
    d2::World::new(d2::WorldConfig {
        max_bodies: 8,
        gravity: Vec2::ZERO,
        max_pairs: 64,
        broadphase_margin: d2::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 64,
        max_joints: 0,
        speculative_margin: d2::DEFAULT_SPECULATIVE_MARGIN,
        solver: d2::SolverConfig::DEFAULT,
        sleep: d2::SleepConfig::DISABLED,
    })
}

fn world3() -> Result<d3::World, PhysicsError> {
    d3::World::new(d3::WorldConfig {
        max_bodies: 8,
        gravity: Vec3::ZERO,
        max_pairs: 64,
        broadphase_margin: d3::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 64,
        max_joints: 0,
        speculative_margin: d3::DEFAULT_SPECULATIVE_MARGIN,
        solver: d3::SolverConfig::DEFAULT,
        sleep: d3::SleepConfig::DISABLED,
    })
}

/// Three boxes in a row, the first moving at the largest given speed, each next three times
/// lighter: refused at the second step (card 8-48).
fn chain2() -> Result<d2::World, PhysicsError> {
    let mut world = world2()?;
    for (index, density) in [9.0, 3.0, 1.0].into_iter().enumerate() {
        let first = index == 0;
        world.create_body(&d2::BodyDesc {
            linear_velocity: Vec2::new(if first { d2::MAX_GIVEN_SPEED } else { 0.0 }, 0.0),
            density,
            restitution: 1.0,
            ..d2::BodyDesc::new(
                d2::BodyKind::Dynamic,
                Vec2::new(SPACING * f32::from(u8::try_from(index).unwrap_or(0)), 0.0),
                d2::Shape2::Box {
                    half_extents: Vec2::new(0.5, 0.5),
                },
            )
        })?;
    }
    Ok(world)
}

fn chain3() -> Result<d3::World, PhysicsError> {
    let mut world = world3()?;
    for (index, density) in [9.0, 3.0, 1.0].into_iter().enumerate() {
        let first = index == 0;
        world.create_body(&d3::BodyDesc {
            linear_velocity: Vec3::new(if first { d3::MAX_GIVEN_SPEED } else { 0.0 }, 0.0, 0.0),
            density,
            restitution: 1.0,
            ..d3::BodyDesc::new(
                d3::BodyKind::Dynamic,
                Vec3::new(
                    SPACING * f32::from(u8::try_from(index).unwrap_or(0)),
                    0.0,
                    0.0,
                ),
                d3::Shape3::Box {
                    half_extents: Vec3::new(0.5, 0.5, 0.5),
                },
            )
        })?;
    }
    Ok(world)
}

#[test]
fn no_body_is_named_before_a_refusal_and_after_a_good_step() -> Result<(), PhysicsError> {
    let mut world = chain2()?;
    assert!(world.diverged_bodies().is_empty());
    assert_eq!(world.step(DT), Ok(()));
    assert!(world.diverged_bodies().is_empty());
    Ok(())
}

/// Two light and two heavy elastic balls, in two pairs far apart, each pair going at each other at
/// the largest given speed: the light ball of each pair leaves above the bound (card 8-47) and the
/// heavy one does not. Creation order: light, heavy, light, heavy.
fn two_pairs2() -> Result<(d2::World, Vec<d2::BodyHandle>), PhysicsError> {
    let mut world = world2()?;
    let mut handles = Vec::new();
    for (x, sign, density) in [
        (0.0, 1.0, 1.0),
        (0.9, -1.0, 1000.0),
        (100.0, 1.0, 1.0),
        (100.9, -1.0, 1000.0),
    ] {
        handles.push(world.create_body(&d2::BodyDesc {
            linear_velocity: Vec2::new(sign * d2::MAX_GIVEN_SPEED, 0.0),
            density,
            restitution: 1.0,
            ..d2::BodyDesc::new(
                d2::BodyKind::Dynamic,
                Vec2::new(x, 0.0),
                d2::Shape2::Circle { radius: 0.5 },
            )
        })?);
    }
    Ok((world, handles))
}

fn two_pairs3() -> Result<(d3::World, Vec<d3::BodyHandle>), PhysicsError> {
    let mut world = world3()?;
    let mut handles = Vec::new();
    for (x, sign, density) in [
        (0.0, 1.0, 1.0),
        (0.9, -1.0, 1000.0),
        (100.0, 1.0, 1.0),
        (100.9, -1.0, 1000.0),
    ] {
        handles.push(world.create_body(&d3::BodyDesc {
            linear_velocity: Vec3::new(sign * d3::MAX_GIVEN_SPEED, 0.0, 0.0),
            density,
            restitution: 1.0,
            ..d3::BodyDesc::new(
                d3::BodyKind::Dynamic,
                Vec3::new(x, 0.0, 0.0),
                d3::Shape3::Sphere { radius: 0.5 },
            )
        })?);
    }
    Ok((world, handles))
}

#[test]
fn a_diverged_step_names_exactly_the_bodies_past_the_bound_in_slot_order_in_2d()
-> Result<(), PhysicsError> {
    let (mut world, handles) = two_pairs2()?;
    assert_eq!(world.step(DT), Err(PhysicsError::Diverged));
    // The two light balls, in the order of their slots; neither heavy ball.
    assert_eq!(world.diverged_bodies(), &[handles[0], handles[2]]);
    Ok(())
}

#[test]
fn a_diverged_step_names_exactly_the_bodies_past_the_bound_in_slot_order_in_3d()
-> Result<(), PhysicsError> {
    let (mut world, handles) = two_pairs3()?;
    assert_eq!(world.step(DT), Err(PhysicsError::Diverged));
    assert_eq!(world.diverged_bodies(), &[handles[0], handles[2]]);
    Ok(())
}

#[test]
fn the_chain_of_boxes_names_only_the_body_that_crossed_the_bound_not_the_fast_one()
-> Result<(), PhysicsError> {
    let mut world = chain2()?;
    let handles: Vec<_> = world.iter().map(|(handle, _)| handle).collect();
    assert_eq!(world.step(DT), Ok(()));
    assert_eq!(world.step(DT), Err(PhysicsError::Diverged));
    assert!(!world.diverged_bodies().contains(&handles[0]));
    assert_eq!(world.diverged_bodies().len(), 1);
    Ok(())
}

#[test]
fn the_list_is_a_snapshot_that_may_name_a_body_destroyed_since() -> Result<(), PhysicsError> {
    let (mut world, handles) = two_pairs2()?;
    assert_eq!(world.step(DT), Err(PhysicsError::Diverged));
    world.destroy_body(handles[0])?;
    assert_eq!(world.diverged_bodies(), &[handles[0], handles[2]]);
    assert_eq!(
        world.body(handles[0]).err(),
        Some(PhysicsError::StaleHandle)
    );
    // A new body does not take its name.
    let fresh = world.create_body(&d2::BodyDesc::new(
        d2::BodyKind::Static,
        Vec2::new(-50.0, 0.0),
        d2::Shape2::Circle { radius: 0.5 },
    ))?;
    assert!(!world.diverged_bodies().contains(&fresh));
    // Changing the policy does not touch it either; only the next real step does.
    world.set_speed_policy(d2::SpeedPolicy::Clamp {
        linear: 500.0,
        angular: 50.0,
    })?;
    assert_eq!(world.diverged_bodies().len(), 2);
    assert_eq!(world.step(DT), Ok(()));
    assert!(world.diverged_bodies().is_empty());
    Ok(())
}

#[test]
fn a_clamping_world_never_refuses_and_names_nobody() -> Result<(), PhysicsError> {
    let (mut world, _) = two_pairs2()?;
    world.set_speed_policy(d2::SpeedPolicy::Clamp {
        linear: 500.0,
        angular: 50.0,
    })?;
    for _ in 0..10 {
        assert_eq!(world.step(DT), Ok(()));
        assert!(world.diverged_bodies().is_empty());
    }
    Ok(())
}

#[test]
fn destroying_the_named_bodies_lets_the_world_step_again() -> Result<(), PhysicsError> {
    let mut world = chain2()?;
    assert_eq!(world.step(DT), Ok(()));
    assert_eq!(world.step(DT), Err(PhysicsError::Diverged));
    let named = world.diverged_bodies().to_vec();
    assert_eq!(named.len(), 1);
    for handle in named {
        world.destroy_body(handle)?;
    }
    assert_eq!(world.len(), 2);
    assert_eq!(world.step(DT), Ok(()));
    let mut world = chain3()?;
    assert_eq!(world.step(DT), Ok(()));
    assert_eq!(world.step(DT), Err(PhysicsError::Diverged));
    for handle in world.diverged_bodies().to_vec() {
        world.destroy_body(handle)?;
    }
    assert_eq!(world.step(DT), Ok(()));
    Ok(())
}

#[test]
fn the_names_are_cleared_by_the_next_step_and_kept_by_a_call_that_is_not_a_step()
-> Result<(), PhysicsError> {
    let mut world = chain2()?;
    assert_eq!(world.step(DT), Ok(()));
    assert_eq!(world.step(DT), Err(PhysicsError::Diverged));
    assert!(!world.diverged_bodies().is_empty());
    // A time step that is not valid is not a step (it changes nothing at all), so it keeps them.
    let named = world.diverged_bodies().to_vec();
    assert_eq!(world.step(-1.0), Err(PhysicsError::InvalidTimeStep));
    assert_eq!(world.diverged_bodies(), named.as_slice());
    assert_eq!(world.step(DT), Err(PhysicsError::Diverged));
    assert_eq!(world.diverged_bodies(), named.as_slice());
    for handle in world.diverged_bodies().to_vec() {
        world.destroy_body(handle)?;
    }
    assert_eq!(world.step(DT), Ok(()));
    assert!(world.diverged_bodies().is_empty());
    Ok(())
}

#[test]
fn a_value_that_overflows_names_its_body_though_it_is_not_a_speed_past_the_bound()
-> Result<(), PhysicsError> {
    // Card 8-38 (owner's choice): a step refused as `NotFinite` names the bodies whose new state
    // is not finite or cannot be held, like `Diverged` does; before it named nobody.
    let mut world = world2()?;
    let body = world.create_body(&d2::BodyDesc {
        linear_velocity: Vec2::new(d2::MAX_GIVEN_SPEED, 0.0),
        ..d2::BodyDesc::new(
            d2::BodyKind::Dynamic,
            Vec2::new(f32::MAX / 2.0, 0.0),
            d2::Shape2::Circle { radius: 0.5 },
        )
    })?;
    assert_eq!(world.step(1.0e35), Err(PhysicsError::NotFinite));
    assert_eq!(world.diverged_bodies(), &[body]);
    Ok(())
}

/// A light ball (about 3e-7 kg in 2D, 4e-9 kg in 3D) between two ordinary ones, far apart: a force
/// of `f32::MAX` on it is a finite number that `apply_force` accepts, and overflows in the step.
fn light_middle2() -> Result<(d2::World, Vec<d2::BodyHandle>), PhysicsError> {
    let mut world = world2()?;
    let mut handles = Vec::new();
    for (index, (radius, density)) in [(0.5, 1.0), (0.01, 0.001), (0.5, 1.0)]
        .into_iter()
        .enumerate()
    {
        handles.push(world.create_body(&d2::BodyDesc {
            density,
            ..d2::BodyDesc::new(
                d2::BodyKind::Dynamic,
                Vec2::new(10.0 * f32::from(u8::try_from(index).unwrap_or(0)), 0.0),
                d2::Shape2::Circle { radius },
            )
        })?);
    }
    Ok((world, handles))
}

fn light_middle3() -> Result<(d3::World, Vec<d3::BodyHandle>), PhysicsError> {
    let mut world = world3()?;
    let mut handles = Vec::new();
    for (index, (radius, density)) in [(0.5, 1.0), (0.01, 0.001), (0.5, 1.0)]
        .into_iter()
        .enumerate()
    {
        handles.push(world.create_body(&d3::BodyDesc {
            density,
            ..d3::BodyDesc::new(
                d3::BodyKind::Dynamic,
                Vec3::new(10.0 * f32::from(u8::try_from(index).unwrap_or(0)), 0.0, 0.0),
                d3::Shape3::Sphere { radius },
            )
        })?);
    }
    Ok((world, handles))
}

#[test]
fn a_huge_force_names_the_body_it_was_applied_to() -> Result<(), PhysicsError> {
    let (mut world, handles) = light_middle2()?;
    world.apply_force(handles[1], Vec2::new(f32::MAX, 0.0))?;
    assert_eq!(world.step(DT), Err(PhysicsError::NotFinite));
    assert_eq!(world.diverged_bodies(), &[handles[1]]);
    let (mut world, handles) = light_middle3()?;
    world.apply_force(handles[1], Vec3::new(f32::MAX, 0.0, 0.0))?;
    assert_eq!(world.step(DT), Err(PhysicsError::NotFinite));
    assert_eq!(world.diverged_bodies(), &[handles[1]]);
    Ok(())
}

#[test]
fn a_huge_torque_names_the_body_it_was_applied_to() -> Result<(), PhysicsError> {
    let (mut world, handles) = light_middle2()?;
    world.apply_torque(handles[1], f32::MAX)?;
    assert_eq!(world.step(DT), Err(PhysicsError::NotFinite));
    assert_eq!(world.diverged_bodies(), &[handles[1]]);
    let (mut world, handles) = light_middle3()?;
    world.apply_torque(handles[1], Vec3::new(0.0, 0.0, f32::MAX))?;
    assert_eq!(world.step(DT), Err(PhysicsError::NotFinite));
    assert_eq!(world.diverged_bodies(), &[handles[1]]);
    Ok(())
}

#[test]
fn the_world_steps_again_once_the_body_named_for_a_huge_force_is_destroyed()
-> Result<(), PhysicsError> {
    let (mut world, handles) = light_middle2()?;
    world.apply_force(handles[1], Vec2::new(f32::MAX, 0.0))?;
    assert_eq!(world.step(DT), Err(PhysicsError::NotFinite));
    for handle in world.diverged_bodies().to_vec() {
        world.destroy_body(handle)?;
    }
    assert_eq!(world.step(DT), Ok(()));
    assert!(world.diverged_bodies().is_empty());
    let (mut world, handles) = light_middle3()?;
    world.apply_force(handles[1], Vec3::new(f32::MAX, 0.0, 0.0))?;
    assert_eq!(world.step(DT), Err(PhysicsError::NotFinite));
    for handle in world.diverged_bodies().to_vec() {
        world.destroy_body(handle)?;
    }
    assert_eq!(world.step(DT), Ok(()));
    assert!(world.diverged_bodies().is_empty());
    Ok(())
}

#[test]
fn a_good_step_after_a_refusal_for_a_huge_force_names_nobody() -> Result<(), PhysicsError> {
    // The refused step dropped every force, so the next one is an ordinary step.
    let (mut world, handles) = light_middle2()?;
    world.apply_force(handles[1], Vec2::new(f32::MAX, 0.0))?;
    assert_eq!(world.step(DT), Err(PhysicsError::NotFinite));
    assert_eq!(world.step(DT), Ok(()));
    assert!(world.diverged_bodies().is_empty());
    let (mut world, handles) = light_middle3()?;
    world.apply_force(handles[1], Vec3::new(f32::MAX, 0.0, 0.0))?;
    assert_eq!(world.step(DT), Err(PhysicsError::NotFinite));
    assert_eq!(world.step(DT), Ok(()));
    assert!(world.diverged_bodies().is_empty());
    Ok(())
}
