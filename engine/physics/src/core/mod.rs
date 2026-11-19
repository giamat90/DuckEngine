//! The dimension-generic core: world, bodies, handles, the narrow [`Dim`] trait and the state hash.
//! Nothing here knows how many dimensions there are; the instance (`d2`, later `d3`) supplies them.

pub mod body;
pub mod bounds;
mod broadphase;
pub(crate) mod cells;
pub mod colouring;
pub mod debug;
pub mod dim;
pub mod error;
pub mod execution;
pub mod hash;
mod islands;
pub mod joint;
pub mod manifolds;
pub mod pairs;
pub mod sleep;
pub(crate) mod soft;
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
pub use debug::{DebugColour, NORMAL_LENGTH};
pub use dim::{Dim, Magnitude, MassProperties, Placed, Quantity};
pub use error::PhysicsError;
pub use execution::{DEFAULT_CHUNK, SolverExecution};
pub use hash::{HashBits, StateHasher};
pub use joint::{JointDesc, JointEntry, JointHandle, JointShape, JointTag, MAX_JOINTS, NoJoint};
pub use manifolds::{ContactManifold, ManifoldList};
pub use pairs::{Pair, PairList};
pub use sleep::{
    DEFAULT_SLEEP_ANGULAR_SPEED, DEFAULT_SLEEP_LINEAR_SPEED, DEFAULT_TIME_TO_SLEEP, SleepConfig,
};
pub use solver::{
    ContactSolver, DEFAULT_CONTACT_DAMPING_RATIO, DEFAULT_CONTACT_HERTZ,
    DEFAULT_CONTACT_PUSH_SPEED, DEFAULT_JOINT_DAMPING_RATIO, DEFAULT_JOINT_HERTZ,
    DEFAULT_LINEAR_SLOP, DEFAULT_RELAX_ITERATIONS, DEFAULT_RESTITUTION_THRESHOLD,
    DEFAULT_SUB_STEPS, FreeSolver, MAX_RELAX_ITERATIONS, MAX_SUB_STEPS, SolverConfig, SolverStats,
    StepFrame,
};
pub use tree::{ProxyId, Tree};
pub use world::{DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, World, WorldConfig};

/// The most bodies one world may be built for (see [`PhysicsError::CapacityTooLarge`]); the limit
/// keeps world creation from asking the allocator for an absurd block, which would abort.
pub const MAX_BODIES: u32 = 1 << 20;

/// The fastest linear speed, metres per second, a body may have after a step; a step that would
/// give a body more is refused with [`PhysicsError::Diverged`] (card 8-25), and a speed above it given
/// to `create_body` or a velocity setter is refused with [`PhysicsError::SpeedTooHigh`] (card 8-36). A bullet is
/// about a thousand metres per second: no scene of the engine comes near this bound, a diverging
/// joint scene passes it within a few dozen steps.
pub const MAX_SPEED: f32 = 1.0e5;

/// The fastest angular speed, radians per second, a body may have after a step (see [`MAX_SPEED`]).
pub const MAX_SPIN: f32 = 1.0e5;

/// The most pairs one world may hold in its pair list (see [`PhysicsError::PairCapacityTooLarge`]).
pub const MAX_PAIRS: u32 = 1 << 24;
