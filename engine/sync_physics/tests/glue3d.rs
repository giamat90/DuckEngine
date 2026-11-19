//! Card 8-10: the 3D glue. Forces in, poses out, bodies attached and released, on a 3D world.

use base::math::{Quat, Vec3};
use ecs::{Entity, World};
use physics::d3::{
    BodyDesc, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, STANDARD_GRAVITY,
    Shape3, SleepConfig, SolverConfig, World as PhysicsWorld, WorldConfig,
};
use scene::{Transform3D, set_parent};
use sync_physics::d3::{
    AppliedForce, PhysicsBody, attach_body, pull_transforms, push_forces, release_body,
};
use sync_physics::{SyncError, SyncReport};

const DT: f32 = 1.0 / 60.0;

fn physics(gravity: Vec3, max_bodies: u32) -> Result<PhysicsWorld, SyncError> {
    Ok(PhysicsWorld::new(WorldConfig {
        max_bodies,
        gravity,
        max_pairs: 64,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 64,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: SleepConfig::DEFAULT,
    })?)
}

fn entity(world: &mut World) -> Result<Entity, SyncError> {
    let entity = world.spawn()?;
    world.insert(entity, Transform3D::IDENTITY)?;
    Ok(entity)
}

fn ball(at: Vec3) -> BodyDesc {
    BodyDesc::new(BodyKind::Dynamic, at, Shape3::Sphere { radius: 0.5 })
}

fn floor() -> BodyDesc {
    BodyDesc::new(
        BodyKind::Static,
        Vec3::new(0.0, -0.5, 0.0),
        Shape3::Box {
            half_extents: Vec3::new(10.0, 0.5, 10.0),
        },
    )
}

fn pose(world: &World, e: Entity) -> Result<Transform3D, SyncError> {
    world
        .get::<Transform3D>(e)
        .copied()
        .ok_or(SyncError::NotAttached)
}

#[test]
fn attaching_gives_the_entity_a_body_and_the_pose_of_the_description() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(STANDARD_GRAVITY, 4)?;
    let e = entity(&mut world)?;
    world.insert(
        e,
        Transform3D {
            scale: Vec3::new(2.0, 3.0, 4.0),
            ..Transform3D::IDENTITY
        },
    )?;
    let desc = BodyDesc {
        rotation: Quat::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), 0.5).unwrap_or(Quat::IDENTITY),
        ..ball(Vec3::new(1.0, 2.0, 3.0))
    };
    let handle = attach_body(&mut world, &mut phys, e, &desc)?;
    let body = world.get::<PhysicsBody>(e).map(PhysicsBody::handle);
    assert_eq!(body, Some(handle));
    assert!(phys.body(handle).is_ok());
    let t = pose(&world, e)?;
    assert_eq!(t.position, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(t.rotation, desc.rotation);
    assert_eq!(
        t.scale,
        Vec3::new(2.0, 3.0, 4.0),
        "an existing scale is kept"
    );
    Ok(())
}

#[test]
fn attaching_is_refused_for_a_parent_a_second_body_and_a_stale_entity() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(STANDARD_GRAVITY, 4)?;
    let (parent, child) = (entity(&mut world)?, entity(&mut world)?);
    set_parent(&mut world, child, parent).map_err(|_| SyncError::NotAttached)?;
    assert_eq!(
        attach_body(&mut world, &mut phys, child, &ball(Vec3::ZERO)).map(|_| ()),
        Err(SyncError::HasParent)
    );
    attach_body(&mut world, &mut phys, parent, &ball(Vec3::ZERO))?;
    assert_eq!(
        attach_body(&mut world, &mut phys, parent, &ball(Vec3::ZERO)).map(|_| ()),
        Err(SyncError::AlreadyAttached)
    );
    let gone = entity(&mut world)?;
    world.despawn(gone)?;
    assert!(matches!(
        attach_body(&mut world, &mut phys, gone, &ball(Vec3::ZERO)),
        Err(SyncError::Ecs(_))
    ));
    Ok(())
}

#[test]
fn a_refused_description_changes_nothing() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(STANDARD_GRAVITY, 4)?;
    let e = entity(&mut world)?;
    let bad = BodyDesc {
        density: -1.0,
        ..ball(Vec3::ZERO)
    };
    assert!(matches!(
        attach_body(&mut world, &mut phys, e, &bad),
        Err(SyncError::Physics(_))
    ));
    assert!(!world.has::<PhysicsBody>(e));
    assert_eq!(pose(&world, e)?, Transform3D::IDENTITY);
    Ok(())
}

#[test]
fn releasing_removes_the_body_and_the_component() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(STANDARD_GRAVITY, 4)?;
    let e = entity(&mut world)?;
    let handle = attach_body(&mut world, &mut phys, e, &ball(Vec3::ZERO))?;
    release_body(&mut world, &mut phys, e)?;
    assert!(!world.has::<PhysicsBody>(e));
    assert!(phys.body(handle).is_err());
    assert_eq!(
        release_body(&mut world, &mut phys, e),
        Err(SyncError::NotAttached)
    );
    Ok(())
}

#[test]
fn a_falling_body_moves_its_transform_down() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(STANDARD_GRAVITY, 4)?;
    let e = entity(&mut world)?;
    attach_body(&mut world, &mut phys, e, &ball(Vec3::new(0.0, 5.0, 0.0)))?;
    for _ in 0..30 {
        phys.step(DT)?;
    }
    assert_eq!(
        pull_transforms(&mut world, &phys)?,
        SyncReport {
            synced: 1,
            skipped: 0
        }
    );
    assert!(pose(&world, e)?.position.y < 4.5);
    Ok(())
}

#[test]
fn a_spinning_body_writes_its_orientation_and_the_scale_stays() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec3::ZERO, 4)?;
    let e = entity(&mut world)?;
    let desc = BodyDesc {
        angular_velocity: Vec3::new(0.0, 3.0, 0.0),
        shape: Shape3::Box {
            half_extents: Vec3::new(0.5, 0.5, 0.5),
        },
        ..ball(Vec3::ZERO)
    };
    let handle = attach_body(&mut world, &mut phys, e, &desc)?;
    world.insert(
        e,
        Transform3D {
            scale: Vec3::new(4.0, 5.0, 6.0),
            ..pose(&world, e)?
        },
    )?;
    for _ in 0..20 {
        phys.step(DT)?;
    }
    pull_transforms(&mut world, &phys)?;
    let t = pose(&world, e)?;
    let body = phys.body(handle)?;
    assert_eq!(t.rotation, body.rotation());
    assert_eq!(t.position, body.position());
    assert!((t.rotation.length() - 1.0).abs() < 1e-4);
    assert!(t.rotation.y.abs() > 0.1, "it turned about Y");
    assert_eq!(t.scale, Vec3::new(4.0, 5.0, 6.0));
    Ok(())
}

#[test]
fn a_force_changes_the_velocity_and_a_zero_force_does_not_wake_a_body() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec3::ZERO, 4)?;
    let (pushed, idle) = (entity(&mut world)?, entity(&mut world)?);
    let h = attach_body(&mut world, &mut phys, pushed, &ball(Vec3::ZERO))?;
    attach_body(&mut world, &mut phys, idle, &ball(Vec3::new(5.0, 0.0, 0.0)))?;
    world.insert(
        pushed,
        AppliedForce {
            force: Vec3::new(10.0, 0.0, 0.0),
            torque: Vec3::new(0.0, 1.0, 0.0),
        },
    )?;
    world.insert(idle, AppliedForce::default())?;
    assert_eq!(
        push_forces(&mut world, &mut phys)?,
        SyncReport {
            synced: 1,
            skipped: 0
        }
    );
    phys.step(DT)?;
    let body = phys.body(h)?;
    assert!(body.linear_velocity().x > 0.0);
    assert!(body.angular_velocity().y > 0.0);
    Ok(())
}

#[test]
fn a_value_that_is_not_finite_and_a_static_body_are_skipped_not_applied() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec3::ZERO, 4)?;
    let (nan, rock) = (entity(&mut world)?, entity(&mut world)?);
    attach_body(&mut world, &mut phys, nan, &ball(Vec3::ZERO))?;
    attach_body(&mut world, &mut phys, rock, &floor())?;
    world.insert(
        nan,
        AppliedForce {
            force: Vec3::new(f32::NAN, 0.0, 0.0),
            torque: Vec3::ZERO,
        },
    )?;
    world.insert(
        rock,
        AppliedForce {
            force: Vec3::new(1.0, 0.0, 0.0),
            torque: Vec3::ZERO,
        },
    )?;
    assert_eq!(
        push_forces(&mut world, &mut phys)?,
        SyncReport {
            synced: 0,
            skipped: 2
        }
    );
    Ok(())
}

#[test]
fn a_body_that_is_gone_is_counted_and_its_transform_left_alone() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(STANDARD_GRAVITY, 4)?;
    let e = entity(&mut world)?;
    let handle = attach_body(&mut world, &mut phys, e, &ball(Vec3::new(1.0, 1.0, 1.0)))?;
    phys.destroy_body(handle)?;
    assert_eq!(
        pull_transforms(&mut world, &phys)?,
        SyncReport {
            synced: 0,
            skipped: 1
        }
    );
    assert_eq!(pose(&world, e)?.position, Vec3::new(1.0, 1.0, 1.0));
    Ok(())
}

#[test]
fn a_ball_dropped_on_a_floor_through_the_glue_comes_to_rest_on_it() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(STANDARD_GRAVITY, 4)?;
    let (ground, e) = (entity(&mut world)?, entity(&mut world)?);
    attach_body(&mut world, &mut phys, ground, &floor())?;
    attach_body(&mut world, &mut phys, e, &ball(Vec3::new(0.0, 3.0, 0.0)))?;
    for _ in 0..240 {
        push_forces(&mut world, &mut phys)?;
        phys.step(DT)?;
        pull_transforms(&mut world, &phys)?;
    }
    let y = pose(&world, e)?.position.y;
    assert!((y - 0.5).abs() < 0.05, "resting height {y}");
    Ok(())
}

#[test]
fn a_non_finite_component_anywhere_in_a_force_or_a_torque_is_skipped() -> Result<(), SyncError> {
    let nan = f32::NAN;
    let bad = [
        (Vec3::new(0.0, nan, 0.0), Vec3::ZERO),
        (Vec3::new(0.0, 0.0, nan), Vec3::ZERO),
        (Vec3::ZERO, Vec3::new(nan, 0.0, 0.0)),
        (Vec3::ZERO, Vec3::new(0.0, nan, 0.0)),
        (Vec3::ZERO, Vec3::new(0.0, 0.0, nan)),
        (Vec3::new(1.0, 0.0, 0.0), Vec3::new(f32::INFINITY, 0.0, 0.0)),
    ];
    for (force, torque) in bad {
        let mut world = World::new();
        let mut phys = physics(Vec3::ZERO, 4)?;
        let e = entity(&mut world)?;
        attach_body(&mut world, &mut phys, e, &ball(Vec3::ZERO))?;
        world.insert(e, AppliedForce { force, torque })?;
        assert_eq!(
            push_forces(&mut world, &mut phys)?,
            SyncReport {
                synced: 0,
                skipped: 1
            },
            "{force:?} {torque:?}"
        );
    }
    Ok(())
}

#[test]
fn a_force_is_not_half_applied_when_its_torque_is_not_finite() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec3::ZERO, 4)?;
    let e = entity(&mut world)?;
    let handle = attach_body(&mut world, &mut phys, e, &ball(Vec3::ZERO))?;
    world.insert(
        e,
        AppliedForce {
            force: Vec3::new(50.0, 0.0, 0.0),
            torque: Vec3::new(f32::NAN, 0.0, 0.0),
        },
    )?;
    assert_eq!(
        push_forces(&mut world, &mut phys)?,
        SyncReport {
            synced: 0,
            skipped: 1
        }
    );
    phys.step(DT)?;
    assert_eq!(
        phys.body(handle)?.linear_velocity(),
        Vec3::ZERO,
        "the finite force was not applied"
    );
    Ok(())
}
