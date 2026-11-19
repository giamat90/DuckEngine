//! Errors of the scene: values, never panics (digest section 4).

use core::fmt;
use ecs::EcsError;

/// Why a scene call was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneError {
    /// The world refused the call (a despawned entity, or a storage limit).
    Ecs(EcsError),
    /// The entity has no local transform, so it cannot take part in the hierarchy.
    MissingTransform,
    /// The new parent is the child or one of its descendants: the hierarchy would loop.
    Cycle,
    /// The child and the parent are not both 2D or both 3D (or one has both transforms): a tree
    /// is all one dimension.
    MixedDimensions,
    /// The links do not form a forest: a link component was removed by hand (`World::remove` or
    /// `Commands::remove`) and the walk met a loop. Nothing was changed beyond what the call had
    /// already written.
    BrokenHierarchy,
}

impl From<EcsError> for SceneError {
    fn from(error: EcsError) -> Self {
        Self::Ecs(error)
    }
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ecs(error) => write!(f, "the world refused the call: {error}"),
            Self::MissingTransform => f.write_str("the entity has no local transform"),
            Self::MixedDimensions => {
                f.write_str("a tree is all 2D or all 3D: the entities differ in dimension")
            }
            Self::Cycle => f.write_str("the parent is the child or one of its descendants"),
            Self::BrokenHierarchy => f.write_str("the hierarchy links contain a loop"),
        }
    }
}

impl std::error::Error for SceneError {}
