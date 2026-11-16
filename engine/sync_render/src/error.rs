//! Errors of the glue: values, never panics.

use core::fmt;
use ecs::EcsError;

/// Why a call of the glue was refused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SyncError {
    /// The ECS refused (the query cannot run).
    Ecs(EcsError),
}

impl From<EcsError> for SyncError {
    fn from(error: EcsError) -> Self {
        Self::Ecs(error)
    }
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ecs(error) => write!(f, "ecs: {error}"),
        }
    }
}

impl std::error::Error for SyncError {}
