//! Layer 2 glue crate, the first of the `sync_*` family (design:
//! `wiki/architecture/ecs-and-sandbox-v0.md` section 4). It is the only code that knows the ECS
//! and the physics world together: a body handle is a component, forces go from the ECS into the
//! physics world before a fixed step, and the poses of the bodies come back into the transforms
//! after it. Physics does not know the ECS. No `unsafe` code.

pub mod attach;
pub mod error;
pub mod sync;

pub use attach::{attach_body, release_body};
pub use error::SyncError;
pub use sync::{AppliedForce, PhysicsBody, SyncReport, pull_transforms, push_forces};
