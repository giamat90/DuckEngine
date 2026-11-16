//! Layer 4 sample (card 7-12): the physics sandbox. Boxes and discs fall in a walled arena, every
//! object is an entity with a transform, a sprite and (for a falling one) a body, and one frame
//! runs the nine phases of the frame plan through the glue crates: `sync_physics`, the scene
//! update, `sync_render` and `sync_audio`. An impact above a speed is a sound cue.
//!
//! Everything that decides what happens is plain code over a render device passed in as a trait
//! object, so the tests run the whole sandbox on the Null device. The old `app_physics_sample`
//! stays as it is: it draws the physics world directly, which is the comparison for the cost of
//! going through the ECS (owner choice O5).

pub mod app;
pub mod cli;
pub mod components;
pub mod error;
pub mod renderer;
pub mod sandbox;
pub mod session;
pub mod spawn;

pub use error::Error;
pub use sandbox::{Draw, FrameReport, Requests, Sandbox};
