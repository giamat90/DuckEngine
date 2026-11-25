//! The render port v0 ([`device::IRenderDevice`]), the resource handles and descriptions it
//! speaks, and the backend-neutral POD [`stream::CommandStream`].
//!
//! Layer 2: depends on `base` only. No graphics API appears here; adapters (layer 3) implement the
//! port. Design: `wiki/architecture/render-port-v0.md`.

pub mod camera;
pub mod caps;
#[cfg(any(test, feature = "contract-tests"))]
pub mod contract;
pub mod desc;
pub mod device;
pub mod dump;
pub mod error;
pub mod graph;
pub mod ids;
pub mod lit;
pub mod mesh;
pub mod mesh_scene;
pub mod mips;
pub mod null;
pub mod scene;
pub mod shapes;
pub mod sprite;
pub mod stream;
