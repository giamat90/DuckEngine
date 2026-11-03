//! The frame benchmark scene of card 3-13: one 2D frame of a fixed number of sprites, built the
//! way a game would build it (sort and batch, stage vertices, record a render graph into a command
//! stream, hand the stream to the Null device). Dev-time only; the engine does not depend on it.
//!
//! The scene is deterministic: a fixed seed of the engine's own generator places the sprites.

mod hash;
mod scene;
mod stats;

pub use hash::fnv1a;
pub use scene::{BATCH_FLOOR, FrameScene, LAYERS, SEED, SPRITES, TEXTURES};
pub use stats::{Summary, median, summarize};
