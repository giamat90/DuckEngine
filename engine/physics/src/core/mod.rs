//! The dimension-generic core: world, bodies, handles, the narrow [`Dim`] trait and the state hash.
//! Nothing here knows how many dimensions there are; the instance (`d2`, later `d3`) supplies them.

pub mod body;
pub mod bounds;
mod broadphase;
pub(crate) mod cells;
pub mod colouring;
pub mod dim;
pub mod error;
pub mod execution;
pub mod hash;
mod islands;
pub mod manifolds;
pub mod pairs;
pub mod sleep;
pub mod solver;
pub(crate) mod sort;
pub mod tree;
pub mod world;

pub(crate) use body::damping_factor;
pub use body::{
    Body, BodyDesc, BodyHandle, BodyKind, BodyTag, DEFAULT_FRICTION, DEFAULT_RESTITUTION, Motion,
};
pub use bounds::Bounds;
pub use colouring::{Colour, Colouring, ConstraintEnds, MAX_COLOURS};
pub use dim::{Dim, Magnitude, MassProperties, Placed, Quantity};
pub use error::PhysicsError;
pub use execution::{DEFAULT_CHUNK, SolverExecution};
pub use hash::{HashBits, StateHasher};
pub use manifolds::{ContactManifold, ManifoldList};
pub use pairs::{Pair, PairList};
pub use sleep::{
    DEFAULT_SLEEP_ANGULAR_SPEED, DEFAULT_SLEEP_LINEAR_SPEED, DEFAULT_TIME_TO_SLEEP, SleepConfig,
};
pub use solver::{
    ContactSolver, DEFAULT_CONTACT_DAMPING_RATIO, DEFAULT_CONTACT_HERTZ,
    DEFAULT_CONTACT_PUSH_SPEED, DEFAULT_LINEAR_SLOP, DEFAULT_RELAX_ITERATIONS,
    DEFAULT_RESTITUTION_THRESHOLD, DEFAULT_SUB_STEPS, FreeSolver, MAX_RELAX_ITERATIONS,
    MAX_SUB_STEPS, SolverConfig, SolverStats, StepFrame,
};
pub use tree::{ProxyId, Tree};
pub use world::{DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, World, WorldConfig};

/// The most bodies one world may be built for (see [`PhysicsError::CapacityTooLarge`]); the limit
/// keeps world creation from asking the allocator for an absurd block, which would abort.
pub const MAX_BODIES: u32 = 1 << 20;

/// The most pairs one world may hold in its pair list (see [`PhysicsError::PairCapacityTooLarge`]).
pub const MAX_PAIRS: u32 = 1 << 24;
