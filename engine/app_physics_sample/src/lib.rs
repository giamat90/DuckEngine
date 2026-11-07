//! Layer 4 sample (card 4-24): boxes and discs fall in a walled arena, stepped by the 2D physics
//! world at a fixed step with an accumulator, drawn through the sprite batcher. The sample
//! implements the physics crate's `IPhysicsDebugDraw` port as thin sprites. The application owns
//! the loop; everything that decides what is drawn is plain code that the tests run on the Null
//! render device.

pub mod app;
pub mod args;
pub mod cli;
pub mod controls;
pub mod error;
pub mod headless;
pub mod mapping;
pub mod session;
pub mod simulation;
pub mod view;

pub use error::Error;
