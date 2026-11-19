//! The 3D copy of the glue (card 8-10): a body handle is a component, forces go from the ECS into
//! a [`physics::d3::World`] before a fixed step and the poses of the bodies come back into the
//! [`scene::Transform3D`] of their entities after it.
//!
//! It repeats the shape of the 2D glue (`crate::attach_body` and the rest) instead of sharing a
//! generic helper: the handle, the description, the torque (a vector, not a number) and the
//! rotation (a quaternion, not an angle) all differ, so a generic version would be about as long
//! as the two copies together and would change the 2D public API. Physics still does not know
//! the ECS, and the 2D and the 3D glue can serve one ECS world with two physics worlds.

use crate::error::SyncError;
use crate::sync::{SyncReport, bump};
use base::math::Vec3;
use ecs::{Entity, Read, World, Write};
use physics::d3::{BodyDesc, BodyHandle, PhysicsError, World as PhysicsWorld};
use scene::{Transform3D, parent_of};

/// The body of an entity: a component that holds the handle. Only [`attach_body`] makes one, so a
/// handle cannot be copied onto another entity by accident.
#[derive(Debug)]
pub struct PhysicsBody {
    handle: BodyHandle,
}

impl PhysicsBody {
    /// The handle of the body in the physics world.
    #[must_use]
    pub fn handle(&self) -> BodyHandle {
        self.handle
    }
}

/// A force and a torque that [`push_forces`] applies at every call, until the component is
/// removed or changed.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct AppliedForce {
    /// Newtons through the centre of mass.
    pub force: Vec3,
    /// Newton metres, about the world axes.
    pub torque: Vec3,
}

fn is_finite(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

/// Applies one [`AppliedForce`] to one body; false when the body refused it.
fn push_one(physics: &mut PhysicsWorld, body: &PhysicsBody, applied: &AppliedForce) -> bool {
    is_finite(applied.force)
        && is_finite(applied.torque)
        && physics.apply_force(body.handle, applied.force).is_ok()
        && physics.apply_torque(body.handle, applied.torque).is_ok()
}

/// Applies the [`AppliedForce`] of every entity with a body to its body, in storage order of the
/// ECS. A force and a torque of zero are not applied (applying one wakes a sleeping body), and
/// such an entity is not counted. A body that is gone, a static body or a value that is not
/// finite is counted in `skipped`, not an error.
///
/// # Errors
/// [`SyncError::Ecs`] when the query cannot run.
pub fn push_forces(world: &mut World, physics: &mut PhysicsWorld) -> Result<SyncReport, SyncError> {
    let mut report = SyncReport::default();
    world.for_each::<(Read<PhysicsBody>, Read<AppliedForce>), _>(|(body, applied)| {
        if applied.force == Vec3::ZERO && applied.torque == Vec3::ZERO {
            return;
        }
        if push_one(physics, body, applied) {
            bump(&mut report.synced);
        } else {
            bump(&mut report.skipped);
        }
    })?;
    Ok(report)
}

/// Writes the position and the orientation of every body to the `Transform3D` of its entity; the
/// scale stays. A body that is gone is counted in `skipped` and its transform is left as it is.
///
/// # Errors
/// [`SyncError::Ecs`] when the query cannot run.
pub fn pull_transforms(world: &mut World, physics: &PhysicsWorld) -> Result<SyncReport, SyncError> {
    let mut report = SyncReport::default();
    world.for_each::<(Read<PhysicsBody>, Write<Transform3D>), _>(
        |(body, transform)| match physics.body(body.handle) {
            Ok(state) => {
                transform.position = state.position();
                transform.rotation = state.rotation();
                bump(&mut report.synced);
            }
            Err(_) => bump(&mut report.skipped),
        },
    )?;
    Ok(report)
}

/// Creates a body from `desc` and gives it to `entity`, with a `Transform3D` at the pose of the
/// description (an existing transform keeps its scale; an entity that also has a `Transform2D` has both and no updater updates it, so give a 3D body no 2D transform). The entity must be alive, have no parent
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
        .get::<Transform3D>(entity)
        .map_or(Vec3::ONE, |transform| transform.scale);
    world.insert(entity, PhysicsBody { handle })?;
    world.insert(
        entity,
        Transform3D {
            position: desc.position,
            rotation: desc.rotation,
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
    match physics.destroy_body(handle) {
        Ok(()) | Err(PhysicsError::StaleHandle) => {}
        Err(error) => return Err(error.into()),
    }
    world.remove::<PhysicsBody>(entity)?;
    Ok(())
}
