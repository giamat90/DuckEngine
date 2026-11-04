//! The one error type of the physics crate. Errors are values: a refused call changes nothing.

use std::fmt;

/// Why a physics call was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PhysicsError {
    /// The world has no free body slot; the capacity is fixed when the world is built.
    CapacityExhausted,
    /// The requested capacity is above [`MAX_BODIES`](crate::core::MAX_BODIES).
    CapacityTooLarge {
        /// The most bodies one world may hold.
        max: u32,
    },
    /// The handle names a body that was destroyed, or one that never existed in this world.
    StaleHandle,
    /// A number that must be finite (NaN or infinity was given).
    NotFinite,
    /// A shape, density or damping outside its range (size or density not above zero, damping
    /// below zero), or one whose mass properties are not representable as `f32`.
    InvalidShape,
    /// The time step is not a finite number above zero.
    InvalidTimeStep,
    /// The call changes the motion of a body that is static.
    StaticBody,
    /// The broadphase margin is not a finite number of zero or more.
    InvalidMargin,
    /// The requested pair capacity is above [`MAX_PAIRS`](crate::core::MAX_PAIRS).
    PairCapacityTooLarge {
        /// The most pairs one world may hold.
        max: u32,
    },
}

impl fmt::Display for PhysicsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CapacityExhausted => f.write_str("the world has no free body slot"),
            Self::CapacityTooLarge { max } => {
                write!(f, "capacity too large, at most {max} bodies allowed")
            }
            Self::StaleHandle => f.write_str("the handle names no live body of this world"),
            Self::NotFinite => f.write_str("a value is not a finite number"),
            Self::InvalidShape => f.write_str("shape, density or damping is out of range"),
            Self::InvalidTimeStep => f.write_str("the time step must be finite and above zero"),
            Self::StaticBody => f.write_str("the body is static and does not move"),
            Self::InvalidMargin => f.write_str("the margin must be finite and zero or more"),
            Self::PairCapacityTooLarge { max } => {
                write!(f, "pair capacity too large, at most {max} pairs allowed")
            }
        }
    }
}

impl std::error::Error for PhysicsError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_has_its_own_readable_message() {
        let cases = [
            (
                PhysicsError::CapacityExhausted,
                "the world has no free body slot",
            ),
            (
                PhysicsError::CapacityTooLarge { max: 7 },
                "capacity too large, at most 7 bodies allowed",
            ),
            (
                PhysicsError::StaleHandle,
                "the handle names no live body of this world",
            ),
            (PhysicsError::NotFinite, "a value is not a finite number"),
            (
                PhysicsError::InvalidShape,
                "shape, density or damping is out of range",
            ),
            (
                PhysicsError::InvalidTimeStep,
                "the time step must be finite and above zero",
            ),
            (
                PhysicsError::StaticBody,
                "the body is static and does not move",
            ),
            (
                PhysicsError::InvalidMargin,
                "the margin must be finite and zero or more",
            ),
            (
                PhysicsError::PairCapacityTooLarge { max: 9 },
                "pair capacity too large, at most 9 pairs allowed",
            ),
        ];
        for (error, message) in cases {
            assert_eq!(error.to_string(), message);
        }
    }
}
