//! Layer 4 sample (card 8-11): the 3D sandbox. Spheres, boxes and capsules fall in a walled box,
//! every object is an entity with a transform, a mesh and a body, and one frame runs the nine
//! phases of the frame plan through the glue crates (`sync_physics::d3`, the 3D scene update and
//! `sync_render::d3`). The scene is drawn lit, seen from an orbit camera.
//!
//! Everything that decides what happens is plain code over a render device passed in as a trait
//! object, so the tests run the whole sample on the Null device.

pub mod app;
pub mod cli;
pub mod controls;
pub mod error;
pub mod mesh;
pub mod orbit;
pub mod renderer;
pub mod sandbox;
pub mod session;
pub mod spawn;

pub use error::Error;
