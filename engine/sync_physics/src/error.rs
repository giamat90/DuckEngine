//! Errors of the glue: values, never panics.

use core::fmt;
use ecs::EcsError;
use physics::d2::PhysicsError;

/// Why a call of the glue was refused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SyncError {
    /// The ECS refused (a stale entity, a full world).
    Ecs(EcsError),
    /// The physics world refused (a full world, a bad description).
    Physics(PhysicsError),
    /// The entity has a parent: a body has a pose in the world, a local transform under a parent
    /// would be wrong, so bodies belong on root entities.
    HasParent,
    /// The entity has a body already.
    AlreadyAttached,
    /// The entity has no body.
    NotAttached,
}

impl From<EcsError> for SyncError {
    fn from(error: EcsError) -> Self {
        Self::Ecs(error)
    }
}

impl From<PhysicsError> for SyncError {
    fn from(error: PhysicsError) -> Self {
        Self::Physics(error)
    }
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ecs(error) => write!(f, "ecs: {error}"),
            Self::Physics(error) => write!(f, "physics: {error}"),
            Self::HasParent => {
                f.write_str("a body belongs on a root entity, this one has a parent")
            }
            Self::AlreadyAttached => f.write_str("the entity has a body already"),
            Self::NotAttached => f.write_str("the entity has no body"),
        }
    }
}

impl std::error::Error for SyncError {}
