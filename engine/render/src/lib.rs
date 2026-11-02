//! The render port v0 ([`device::IRenderDevice`]), the resource handles and descriptions it
//! speaks, and the backend-neutral POD [`stream::CommandStream`].
//!
//! Layer 2: depends on `base` only. No graphics API appears here; adapters (layer 3) implement the
//! port. Design: `wiki/architecture/render-port-v0.md`.

pub mod caps;
#[cfg(any(test, feature = "contract-tests"))]
pub mod contract;
pub mod desc;
pub mod device;
pub mod dump;
pub mod error;
pub mod ids;
pub mod null;
pub mod stream;
