//! Throw-away harness of the ADR-0003 spike (card 7-2): four ECS storages built from `std` only,
//! each driven through the same [`Store`] workload API on the shape of the phase 7 sandbox
//! (design: `wiki/architecture/ecs-and-sandbox-v0.md` section 3, evidence:
//! `wiki/spikes/ecs-storage-spike.md`). Report only; not part of the engine.
//!
//! The storages:
//! - [`ArchetypeSafe`]: one dense table per set of components, columns are typed vectors behind a
//!   trait object (no `unsafe`);
//! - [`ArchetypeRaw`]: the same tables with type-erased byte columns ([`raw_column`], the only
//!   `unsafe` of the crate);
//! - [`SparseWorld`]: one sparse set per component;
//! - [`Hybrid`]: archetype tables for the stable components, a sparse set for the component
//!   that is added and removed often.
#![expect(
    unsafe_code,
    reason = "type-erased column of the archetype variant, one module, every block documented"
)]

pub mod archetype;
pub mod column;
pub mod components;
pub mod entity;
pub mod raw_column;
pub mod sparse;
pub mod store;
pub mod workloads;

pub use archetype::{ArchWorld, ArchetypeRaw, ArchetypeSafe, Hybrid};
pub use components::Kind;
pub use entity::Entity;
pub use sparse::SparseWorld;
pub use store::Store;
