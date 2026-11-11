//! Layer 4 sample (card 6-11): a spinning textured cube with a depth buffer, drawn through the
//! mesh frontend of the render crate on the OpenGL or the Vulkan device (`--backend`). The
//! application owns the loop; what is drawn is in [`mesh_sample`], which the tests run on the Null
//! device.

pub mod app;
pub mod mesh_sample;

pub use mesh_sample::{MESH_GLSL, MESH_SPIRV, MeshError, MeshSample};
