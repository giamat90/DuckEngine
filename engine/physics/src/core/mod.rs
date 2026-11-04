//! The dimension-generic core: world, bodies, handles, the narrow [`Dim`] trait and the state hash.
//! Nothing here knows how many dimensions there are; the instance (`d2`, later `d3`) supplies them.

pub mod body;
pub mod dim;
pub mod error;
pub mod hash;
pub mod world;

pub use body::{Body, BodyDesc, BodyHandle, BodyKind, BodyTag};
pub use dim::{Dim, MassProperties, Quantity};
pub use error::PhysicsError;
pub use hash::{HashBits, StateHasher};
pub use world::{World, WorldConfig};

/// The most bodies one world may be built for (see [`PhysicsError::CapacityTooLarge`]); the limit
/// keeps world creation from asking the allocator for an absurd block, which would abort.
pub const MAX_BODIES: u32 = 1 << 20;
