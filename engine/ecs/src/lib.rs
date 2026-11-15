//! Layer 1 ecs crate, part 1 (ADR-0003, design: `wiki/architecture/ecs-and-sandbox-v0.md`
//! section 3): entities as generation-checked handles, components stored in archetype tables with
//! safe typed columns, structural changes and a command buffer. No `unsafe` code, no macros.
//!
//! Queries, declared access and parallel iteration come in the next card.

mod archetype;
mod column;
pub mod component;
pub mod entity;
pub mod error;
pub mod world;

pub use component::{Component, ComponentId};
pub use entity::Entity;
pub use error::EcsError;
pub use world::World;
pub mod commands;
pub mod query;

pub use commands::{ApplyReport, Commands, Target};
pub use query::{Access, Query, Read, Write};
