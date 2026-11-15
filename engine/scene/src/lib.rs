//! Layer 2 scene crate (design: `wiki/architecture/ecs-and-sandbox-v0.md` section 4): 2D local
//! transforms, the parent-child hierarchy as plain components of the `ecs` crate (a parent link,
//! a first child and a next sibling, so children are visited without searching) and a world
//! transform update that walks the hierarchy depth first, parents before children. Cycles are
//! refused when the parent is set. No dirty flags in this version. No `unsafe` code.

pub mod error;
pub mod hierarchy;
pub mod transform;
pub mod update;

pub use error::SceneError;
pub use hierarchy::{
    Children, FirstChild, NextSibling, Parent, children, despawn_subtree, detach, parent_of,
    set_parent,
};
pub use transform::{Transform2D, WorldTransform2D};
pub use update::{TransformUpdater, UpdateReport};
