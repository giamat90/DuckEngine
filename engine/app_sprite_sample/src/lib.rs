//! Layer 4 sample (card 3-15): a GLFW window and the OpenGL 4.5 device draw a triangle, then
//! sprites, with the golden `sprite.dshd` shader asset, the render crate's sprite batcher and its
//! orthographic camera. The application owns the loop.

pub mod app;
pub mod args;
pub mod cli;
pub mod input;
pub mod looping;
pub mod sample;
pub mod scene;

pub use sample::{GOLDEN_SHADER, Sample, SampleError};
pub use scene::Scene;
