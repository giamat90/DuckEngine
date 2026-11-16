//! The two copies: forces in, poses out.

use crate::common::{DT, circle, entity, ground, physics};
use base::math::Vec2;
use ecs::World;
use scene::Transform2D;
use sync_physics::{
    AppliedForce, SyncError, SyncReport, attach_body, pull_transforms, push_forces,
};

fn pose(world: &World, e: ecs::Entity) -> Result<Transform2D, SyncError> {
    world
        .get::<Transform2D>(e)
        .copied()
        .ok_or(SyncError::NotAttached)
}

#[test]
fn a_falling_body_moves_its_transform_down() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec2::new(0.0, -10.0), 4)?;
    let e = entity(&mut world)?;
    attach_body(&mut world, &mut phys, e, &circle(Vec2::new(0.0, 5.0)))?;
    for _ in 0..30 {
        phys.step(DT)?;
    }
    let report = pull_transforms(&mut world, &phys)?;
    assert_eq!(
        report,
        SyncReport {
            synced: 1,
            skipped: 0
        }
    );
    let y = pose(&world, e)?.position.y;
    assert!(y < 5.0 - 0.5, "y = {y}");
    Ok(())
}

#[test]
fn the_pose_is_the_pose_of_the_body_and_the_scale_stays() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec2::new(0.0, -10.0), 4)?;
    let e = entity(&mut world)?;
    let handle = attach_body(&mut world, &mut phys, e, &circle(Vec2::new(1.0, 2.0)))?;
    world.insert(
        e,
        Transform2D {
            scale: Vec2::new(4.0, 5.0),
            ..pose(&world, e)?
        },
    )?;
    phys.step(DT)?;
    pull_transforms(&mut world, &phys)?;
    let body = phys.body(handle)?;
    let t = pose(&world, e)?;
    assert_eq!(t.position, body.position());
    assert!((t.rotation - body.rotation().angle()).abs() < 1e-6);
    assert_eq!(t.scale, Vec2::new(4.0, 5.0));
    Ok(())
}

#[test]
fn an_applied_force_pushes_the_body_for_one_step_each_call() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec2::ZERO, 4)?;
    let e = entity(&mut world)?;
    let handle = attach_body(&mut world, &mut phys, e, &circle(Vec2::ZERO))?;
    world.insert(
        e,
        AppliedForce {
            force: Vec2::new(10.0, 0.0),
            torque: 0.0,
        },
    )?;
    let report = push_forces(&mut world, &mut phys)?;
    assert_eq!(
        report,
        SyncReport {
            synced: 1,
            skipped: 0
        }
    );
    phys.step(DT)?;
    let after_one = phys.body(handle)?.linear_velocity().x;
    assert!(after_one > 0.0, "{after_one}");
    phys.step(DT)?;
    let after_two = phys.body(handle)?.linear_velocity().x;
    assert!(
        (after_two - after_one).abs() < 1e-6,
        "the force acts for one step only"
    );
    push_forces(&mut world, &mut phys)?;
    phys.step(DT)?;
    assert!(phys.body(handle)?.linear_velocity().x > after_two);
    Ok(())
}

#[test]
fn a_torque_turns_the_body() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec2::ZERO, 4)?;
    let e = entity(&mut world)?;
    let handle = attach_body(&mut world, &mut phys, e, &circle(Vec2::ZERO))?;
    world.insert(
        e,
        AppliedForce {
            force: Vec2::ZERO,
            torque: 5.0,
        },
    )?;
    push_forces(&mut world, &mut phys)?;
    phys.step(DT)?;
    assert!(phys.body(handle)?.angular_velocity() > 0.0);
    Ok(())
}

#[test]
fn a_static_body_and_a_bad_force_are_skipped_not_errors() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec2::ZERO, 4)?;
    let (wall, bad, good) = (
        entity(&mut world)?,
        entity(&mut world)?,
        entity(&mut world)?,
    );
    attach_body(&mut world, &mut phys, wall, &ground(Vec2::ZERO))?;
    attach_body(&mut world, &mut phys, bad, &circle(Vec2::new(5.0, 0.0)))?;
    attach_body(&mut world, &mut phys, good, &circle(Vec2::new(-5.0, 0.0)))?;
    let push = AppliedForce {
        force: Vec2::new(1.0, 0.0),
        torque: 0.0,
    };
    world.insert(wall, push)?;
    world.insert(good, push)?;
    world.insert(
        bad,
        AppliedForce {
            force: Vec2::new(f32::NAN, 0.0),
            torque: 0.0,
        },
    )?;
    let report = push_forces(&mut world, &mut phys)?;
    assert_eq!(
        report,
        SyncReport {
            synced: 1,
            skipped: 2
        }
    );
    Ok(())
}

#[test]
fn a_body_destroyed_behind_the_glue_is_skipped_in_both_copies() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec2::ZERO, 4)?;
    let e = entity(&mut world)?;
    let handle = attach_body(&mut world, &mut phys, e, &circle(Vec2::ZERO))?;
    world.insert(
        e,
        AppliedForce {
            force: Vec2::new(1.0, 0.0),
            torque: 0.0,
        },
    )?;
    phys.destroy_body(handle)?;
    assert_eq!(
        push_forces(&mut world, &mut phys)?,
        SyncReport {
            synced: 0,
            skipped: 1
        }
    );
    assert_eq!(
        pull_transforms(&mut world, &phys)?,
        SyncReport {
            synced: 0,
            skipped: 1
        }
    );
    Ok(())
}

#[test]
fn entities_without_a_body_or_a_force_are_not_touched() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec2::ZERO, 4)?;
    let plain = entity(&mut world)?;
    let moved = Transform2D::from_position(Vec2::new(7.0, 8.0));
    world.insert(plain, moved)?;
    assert_eq!(push_forces(&mut world, &mut phys)?, SyncReport::default());
    assert_eq!(pull_transforms(&mut world, &phys)?, SyncReport::default());
    assert_eq!(pose(&world, plain)?, moved);
    Ok(())
}

#[test]
fn a_body_without_a_force_component_is_pulled_but_not_pushed() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec2::ZERO, 4)?;
    let e = entity(&mut world)?;
    attach_body(&mut world, &mut phys, e, &circle(Vec2::ZERO))?;
    assert_eq!(push_forces(&mut world, &mut phys)?, SyncReport::default());
    assert_eq!(pull_transforms(&mut world, &phys)?.synced, 1);
    Ok(())
}

#[test]
fn a_zero_force_is_not_applied_and_does_not_wake_a_sleeping_body() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec2::ZERO, 4)?;
    let e = entity(&mut world)?;
    let handle = attach_body(&mut world, &mut phys, e, &circle(Vec2::ZERO))?;
    world.insert(e, AppliedForce::default())?;
    for _ in 0..60 {
        phys.step(DT)?;
    }
    assert!(
        phys.is_asleep(handle)?,
        "the body should be asleep after a second at rest"
    );
    assert_eq!(push_forces(&mut world, &mut phys)?, SyncReport::default());
    assert!(phys.is_asleep(handle)?);
    world.insert(
        e,
        AppliedForce {
            force: Vec2::new(1.0, 0.0),
            torque: 0.0,
        },
    )?;
    assert_eq!(push_forces(&mut world, &mut phys)?.synced, 1);
    assert!(!phys.is_asleep(handle)?, "a real force wakes it");
    Ok(())
}

#[test]
fn a_value_that_is_not_finite_applies_nothing_even_when_the_rest_is_fine() -> Result<(), SyncError>
{
    let cases = [
        (Vec2::new(f32::NAN, 1.0), 0.0),
        (Vec2::new(1.0, f32::INFINITY), 0.0),
        (Vec2::new(1.0, 1.0), f32::NAN),
    ];
    for (force, torque) in cases {
        let mut world = World::new();
        let mut phys = physics(Vec2::ZERO, 4)?;
        let e = entity(&mut world)?;
        let handle = attach_body(&mut world, &mut phys, e, &circle(Vec2::ZERO))?;
        world.insert(e, AppliedForce { force, torque })?;
        let report = push_forces(&mut world, &mut phys)?;
        assert_eq!(
            report,
            SyncReport {
                synced: 0,
                skipped: 1
            },
            "{force:?} {torque}"
        );
        phys.step(DT)?;
        let body = phys.body(handle)?;
        assert_eq!(body.linear_velocity(), Vec2::ZERO, "{force:?} {torque}");
        assert!(body.angular_velocity().abs() < f32::EPSILON);
    }
    Ok(())
}
