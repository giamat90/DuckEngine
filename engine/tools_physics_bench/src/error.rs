//! The one error type of the benchmark: a physics error, a pool that could not be built, or a
//! measurement with nothing to measure.

use physics::d2::PhysicsError;
use platform::error::PlatformError;
use std::fmt;

/// Why a benchmark could not produce a number.
#[derive(Debug, Clone, PartialEq)]
pub enum BenchError {
    /// The world refused to be built or to step.
    Physics(PhysicsError),
    /// The thread pool could not be built.
    Pool(PlatformError),
    /// A scheduler refused a call (it was busy or the call was too large).
    Scheduler,
    /// Zero iterations, steps or calls were asked for, so there is nothing to summarize.
    NothingToMeasure,
}

impl fmt::Display for BenchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Physics(error) => write!(f, "physics error: {error:?}"),
            Self::Pool(error) => write!(f, "could not build the pool: {error:?}"),
            Self::Scheduler => write!(f, "the scheduler refused a call"),
            Self::NothingToMeasure => write!(f, "nothing to measure (a count was zero)"),
        }
    }
}

impl From<PhysicsError> for BenchError {
    fn from(error: PhysicsError) -> Self {
        Self::Physics(error)
    }
}

impl From<PlatformError> for BenchError {
    fn from(error: PlatformError) -> Self {
        Self::Pool(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_error_says_what_failed() {
        assert!(
            BenchError::from(PhysicsError::NotFinite)
                .to_string()
                .contains("physics")
        );
        assert!(
            BenchError::from(PlatformError::Unsupported)
                .to_string()
                .contains("pool")
        );
        assert!(BenchError::Scheduler.to_string().contains("scheduler"));
        assert!(BenchError::NothingToMeasure.to_string().contains("nothing"));
    }
}
