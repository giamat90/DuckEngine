//! Giving an entity a body and taking it away.

use crate::common::{circle, entity, physics};
use base::math::Vec2;
use ecs::{EcsError, World};
use scene::{Transform2D, set_parent};
use sync_physics::{PhysicsBody, SyncError, attach_body, release_body};

#[test]
fn attach_makes_a_body_a_component_and_a_transform_from_the_description() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec2::ZERO, 4)?;
    let e = world.spawn()?;
    let mut desc = circle(Vec2::new(3.0, 4.0));
    desc.rotation = physics::d2::Rot2::from_angle(0.5);
    let handle = attach_body(&mut world, &mut phys, e, &desc)?;
    assert!(phys.contains(handle));
    assert_eq!(
        world.get::<PhysicsBody>(e).map(PhysicsBody::handle),
        Some(handle)
    );
    let t = world
        .get::<Transform2D>(e)
        .copied()
        .ok_or(SyncError::NotAttached)?;
    assert_eq!(t.position, Vec2::new(3.0, 4.0));
    assert!((t.rotation - 0.5).abs() < 1e-5);
    assert_eq!(t.scale, Vec2::ONE);
    Ok(())
}

#[test]
fn attach_keeps_the_scale_of_an_existing_transform() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec2::ZERO, 4)?;
    let e = entity(&mut world)?;
    world.insert(
        e,
        Transform2D {
            scale: Vec2::new(2.0, 3.0),
            ..Transform2D::IDENTITY
        },
    )?;
    attach_body(&mut world, &mut phys, e, &circle(Vec2::new(1.0, 1.0)))?;
    let t = world
        .get::<Transform2D>(e)
        .copied()
        .ok_or(SyncError::NotAttached)?;
    assert_eq!(t.scale, Vec2::new(2.0, 3.0));
    assert_eq!(t.position, Vec2::new(1.0, 1.0));
    Ok(())
}

#[test]
fn a_stale_entity_is_refused_and_no_body_is_made() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec2::ZERO, 4)?;
    let e = entity(&mut world)?;
    world.despawn(e)?;
    assert_eq!(
        attach_body(&mut world, &mut phys, e, &circle(Vec2::ZERO)),
        Err(SyncError::Ecs(EcsError::StaleEntity))
    );
    assert_eq!(phys.len(), 0);
    Ok(())
}

#[test]
fn an_entity_with_a_parent_is_refused() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec2::ZERO, 4)?;
    let (parent, child) = (entity(&mut world)?, entity(&mut world)?);
    set_parent(&mut world, child, parent).map_err(|_| SyncError::HasParent)?;
    assert_eq!(
        attach_body(&mut world, &mut phys, child, &circle(Vec2::ZERO)),
        Err(SyncError::HasParent)
    );
    assert_eq!(phys.len(), 0);
    Ok(())
}

#[test]
fn a_second_body_for_one_entity_is_refused() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec2::ZERO, 4)?;
    let e = entity(&mut world)?;
    attach_body(&mut world, &mut phys, e, &circle(Vec2::ZERO))?;
    assert_eq!(
        attach_body(&mut world, &mut phys, e, &circle(Vec2::ZERO)),
        Err(SyncError::AlreadyAttached)
    );
    assert_eq!(phys.len(), 1);
    Ok(())
}

#[test]
fn a_full_physics_world_is_an_error_and_leaves_the_entity_alone() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec2::ZERO, 1)?;
    let (a, b) = (entity(&mut world)?, entity(&mut world)?);
    attach_body(&mut world, &mut phys, a, &circle(Vec2::ZERO))?;
    let refused = attach_body(&mut world, &mut phys, b, &circle(Vec2::ZERO));
    assert!(matches!(refused, Err(SyncError::Physics(_))), "{refused:?}");
    assert!(!world.has::<PhysicsBody>(b));
    Ok(())
}

#[test]
fn release_destroys_the_body_and_removes_the_component() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec2::ZERO, 4)?;
    let e = entity(&mut world)?;
    let handle = attach_body(&mut world, &mut phys, e, &circle(Vec2::ZERO))?;
    release_body(&mut world, &mut phys, e)?;
    assert!(!phys.contains(handle));
    assert!(!world.has::<PhysicsBody>(e));
    assert_eq!(
        release_body(&mut world, &mut phys, e),
        Err(SyncError::NotAttached)
    );
    Ok(())
}

#[test]
fn release_of_a_stale_entity_is_refused() -> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec2::ZERO, 4)?;
    let e = entity(&mut world)?;
    world.despawn(e)?;
    assert_eq!(
        release_body(&mut world, &mut phys, e),
        Err(SyncError::Ecs(EcsError::StaleEntity))
    );
    Ok(())
}

#[test]
fn every_error_has_a_distinct_message() {
    let all = [
        SyncError::HasParent,
        SyncError::AlreadyAttached,
        SyncError::NotAttached,
        SyncError::Ecs(EcsError::StaleEntity),
        SyncError::Physics(physics::d2::PhysicsError::StaleHandle),
    ];
    let texts: Vec<String> = all.iter().map(ToString::to_string).collect();
    for text in &texts {
        assert_eq!(texts.iter().filter(|other| *other == text).count(), 1);
    }
}

#[test]
fn release_after_the_body_was_destroyed_behind_the_glue_still_removes_the_component()
-> Result<(), SyncError> {
    let mut world = World::new();
    let mut phys = physics(Vec2::ZERO, 4)?;
    let e = entity(&mut world)?;
    let handle = attach_body(&mut world, &mut phys, e, &circle(Vec2::ZERO))?;
    phys.destroy_body(handle)?;
    assert_eq!(release_body(&mut world, &mut phys, e), Ok(()));
    assert!(!world.has::<PhysicsBody>(e));
    Ok(())
}
