//! Making and dropping the body of an entity.

use crate::error::SyncError;
use crate::sync::PhysicsBody;
use base::math::Vec2;
use ecs::{Entity, World};
use physics::d2::{BodyDesc, BodyHandle, PhysicsError, World as PhysicsWorld};
use scene::{Transform2D, parent_of};

/// Creates a body from `desc` and gives it to `entity`, with a `Transform2D` at the pose of the
/// description (an existing transform keeps its scale). The entity must be alive, have no parent
/// (a body has a pose in the world) and have no body yet.
///
/// # Errors
/// See [`SyncError`]. Nothing is changed on an error: a body already made is destroyed again.
pub fn attach_body(
    world: &mut World,
    physics: &mut PhysicsWorld,
    entity: Entity,
    desc: &BodyDesc,
) -> Result<BodyHandle, SyncError> {
    if !world.is_alive(entity) {
        return Err(SyncError::Ecs(ecs::EcsError::StaleEntity));
    }
    if parent_of(world, entity).is_some() {
        return Err(SyncError::HasParent);
    }
    if world.has::<PhysicsBody>(entity) {
        return Err(SyncError::AlreadyAttached);
    }
    let handle = physics.create_body(desc)?;
    match give_body(world, entity, handle, desc) {
        Ok(()) => Ok(handle),
        Err(error) => {
            // Undo, so that a refused call changes nothing. The body was made a moment ago, so
            // destroying it cannot fail; the component may or may not have been added.
            let _ = world.remove::<PhysicsBody>(entity);
            let _ = physics.destroy_body(handle);
            Err(error)
        }
    }
}

fn give_body(
    world: &mut World,
    entity: Entity,
    handle: BodyHandle,
    desc: &BodyDesc,
) -> Result<(), SyncError> {
    let scale = world
        .get::<Transform2D>(entity)
        .map_or(Vec2::ONE, |transform| transform.scale);
    world.insert(entity, PhysicsBody { handle })?;
    world.insert(
        entity,
        Transform2D {
            position: desc.position,
            rotation: desc.rotation.angle(),
            scale,
        },
    )?;
    Ok(())
}

/// Destroys the body of `entity` and removes the component. A body that was destroyed behind the
/// glue is not an error here: the component goes either way. Despawning an entity with
/// `World::despawn` does not destroy its body (it keeps simulating and keeps its slot): call this
/// first. A handle belongs to the physics world that made it; pass the same world every time.
///
/// # Errors
/// [`SyncError::Ecs`] for a stale entity, [`SyncError::NotAttached`] without a body, and
/// [`SyncError::Physics`] for any other refusal of the physics world.
pub fn release_body(
    world: &mut World,
    physics: &mut PhysicsWorld,
    entity: Entity,
) -> Result<(), SyncError> {
    if !world.is_alive(entity) {
        return Err(SyncError::Ecs(ecs::EcsError::StaleEntity));
    }
    let handle = world
        .get::<PhysicsBody>(entity)
        .map(PhysicsBody::handle)
        .ok_or(SyncError::NotAttached)?;
    // The body goes first: if the physics world refuses for another reason than a stale handle,
    // the component stays and nothing is lost track of.
    match physics.destroy_body(handle) {
        Ok(()) | Err(PhysicsError::StaleHandle) => {}
        Err(error) => return Err(error.into()),
    }
    world.remove::<PhysicsBody>(entity)?;
    Ok(())
}
