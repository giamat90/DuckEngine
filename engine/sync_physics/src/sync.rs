//! The two copies of the glue: ECS to physics before a step, physics to ECS after it.

use crate::error::SyncError;
use base::math::Vec2;
use ecs::{Read, World, Write};
use physics::d2::{BodyHandle, World as PhysicsWorld};
use scene::Transform2D;

/// The body of an entity: a component that holds the handle. Only [`crate::attach_body`] makes
/// one, so a handle cannot be copied onto another entity by accident.
#[derive(Debug)]
pub struct PhysicsBody {
    pub(crate) handle: BodyHandle,
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
    pub force: Vec2,
    /// Newton metres.
    pub torque: f32,
}

/// What a copy did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SyncReport {
    /// Entities copied.
    pub synced: u32,
    /// Entities passed over: a body that is gone, a static body, or a value that is not finite.
    pub skipped: u32,
}

/// Adds one to a counter without overflow.
fn bump(counter: &mut u32) {
    *counter = counter.saturating_add(1);
}

/// True when the force and the torque are numbers a body can take.
fn is_finite(applied: &AppliedForce) -> bool {
    applied.force.x.is_finite() && applied.force.y.is_finite() && applied.torque.is_finite()
}

/// Applies one [`AppliedForce`] to one body; false when the body refused it.
fn push_one(physics: &mut PhysicsWorld, body: &PhysicsBody, applied: &AppliedForce) -> bool {
    is_finite(applied)
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
        if applied.force == Vec2::ZERO && applied.torque == 0.0 {
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

/// Writes the position and the rotation of every body to the `Transform2D` of its entity; the
/// scale stays. A body that is gone is counted in `skipped` and its transform is left as it is.
///
/// # Errors
/// [`SyncError::Ecs`] when the query cannot run.
pub fn pull_transforms(world: &mut World, physics: &PhysicsWorld) -> Result<SyncReport, SyncError> {
    let mut report = SyncReport::default();
    world.for_each::<(Read<PhysicsBody>, Write<Transform2D>), _>(
        |(body, transform)| match physics.body(body.handle) {
            Ok(state) => {
                transform.position = state.position();
                transform.rotation = state.rotation().angle();
                bump(&mut report.synced);
            }
            Err(_) => bump(&mut report.skipped),
        },
    )?;
    Ok(report)
}
