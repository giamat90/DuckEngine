//! What can go wrong in the sample, as values.

use app_sprite_sample::SampleError;
use frame::FixedStepError;
use physics::d2::PhysicsError;
use std::fmt;

/// Why the sample could not build or advance a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The physics world refused something.
    Physics(PhysicsError),
    /// The fixed-step accumulator could not be built.
    Clock(FixedStepError),
    /// The render side failed (device, batcher, graph or shader asset).
    Render(SampleError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Physics(error) => write!(f, "physics: {error}"),
            Self::Clock(error) => write!(f, "fixed step: {error:?}"),
            Self::Render(error) => write!(f, "render: {error}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<PhysicsError> for Error {
    fn from(error: PhysicsError) -> Self {
        Self::Physics(error)
    }
}

impl From<FixedStepError> for Error {
    fn from(error: FixedStepError) -> Self {
        Self::Clock(error)
    }
}

impl From<SampleError> for Error {
    fn from(error: SampleError) -> Self {
        Self::Render(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_source_keeps_its_cause_and_names_its_layer() {
        let physics = Error::from(PhysicsError::StaleHandle);
        assert_eq!(physics, Error::Physics(PhysicsError::StaleHandle));
        assert!(physics.to_string().starts_with("physics: "));
        let clock = Error::from(FixedStepError::ZeroStep);
        assert_eq!(clock.to_string(), "fixed step: ZeroStep");
        let render = Error::from(SampleError::NoCamera);
        assert_eq!(render.to_string(), "render: no camera for this view size");
    }
}
