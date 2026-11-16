//! Errors of the glue: values, never panics.

use core::fmt;

/// Why a call of the glue was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncError {
    /// The cue queue is full; the cue was not kept.
    QueueFull,
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueueFull => f.write_str("the cue queue is full"),
        }
    }
}

impl std::error::Error for SyncError {}
