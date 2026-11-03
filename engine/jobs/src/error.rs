//! The one error type of the job system.

use std::fmt;

/// Why a scheduler call was refused. Errors are values: a refused call runs no job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum JobError {
    /// `grain` was zero, so no chunk size can be derived.
    InvalidGrain,
    /// The call needs more chunks (or tasks) than the scheduler was built to order.
    TooManyChunks {
        /// The most chunks one call may have.
        max: u32,
    },
    /// The scheduler is running another call (a nested one, or one from another thread) and
    /// cannot start this one.
    Busy,
}

impl fmt::Display for JobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidGrain => f.write_str("grain must be at least 1"),
            Self::TooManyChunks { max } => write!(f, "too many chunks, at most {max} allowed"),
            Self::Busy => f.write_str("scheduler is busy with another call"),
        }
    }
}

impl std::error::Error for JobError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_has_its_own_readable_message() {
        let cases = [
            (JobError::InvalidGrain, "grain must be at least 1"),
            (
                JobError::TooManyChunks { max: 8 },
                "too many chunks, at most 8 allowed",
            ),
            (JobError::Busy, "scheduler is busy with another call"),
        ];
        for (error, message) in cases {
            assert_eq!(error.to_string(), message);
        }
    }
}
