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
    /// The requested manifold capacity is above [`MAX_PAIRS`](crate::core::MAX_PAIRS).
    ManifoldCapacityTooLarge {
        /// The most manifolds one world may hold.
        max: u32,
    },
    /// A friction below zero, or a restitution outside zero to one (a number that is not finite is
    /// [`PhysicsError::NotFinite`]).
    InvalidMaterial,
    /// A solver setting outside its range (see [`SolverConfig::validate`](crate::core::SolverConfig::validate)).
    InvalidSolverConfig,
    /// A sleep setting outside its range (see [`SleepConfig::validate`](crate::core::SleepConfig::validate)).
    InvalidSleepConfig,
    /// The world has no free joint slot; the joint capacity is fixed when the world is built.
    JointCapacityExhausted,
    /// The requested joint capacity is above [`MAX_JOINTS`](crate::core::MAX_JOINTS).
    JointCapacityTooLarge {
        /// The most joints one world may hold.
        max: u32,
    },
    /// The handle names a joint that was destroyed (also with one of its bodies), or one that never
    /// existed in this world.
    StaleJoint,
    /// A joint description outside its ranges: the same body twice, two static bodies, an angle
    /// limit that is not an ordered pair inside minus pi to pi, a length that is not above zero, a
    /// spring below zero and so on (a number that is not finite is [`PhysicsError::NotFinite`]).
    InvalidJoint,
    /// A step would give a body a linear speed above [`MAX_SPEED`](crate::core::MAX_SPEED) or an
    /// angular speed above [`MAX_SPIN`](crate::core::MAX_SPIN): the simulation has diverged (a body
    /// tiny for its joint arm, a mass ratio far beyond the stated limits, a huge force). The step
    /// dropped the forces and torques applied for it and changed nothing else, so a scene that
    /// diverges by itself gives the same answer again until it is changed: set the body's velocity
    /// down ([`World::set_linear_velocity`](crate::core::World::set_linear_velocity)) or destroy
    /// a body. The bodies that crossed the bound are
    /// [`World::diverged_bodies`](crate::core::World::diverged_bodies); destroying them was enough in
    /// the chains tested (setting one down may not be enough when its neighbours keep the same contact, and
    /// destroying the fast body that started the chain does not help, card 8-48), or choose
    /// [`SpeedPolicy::Clamp`](crate::core::SpeedPolicy::Clamp) so the step does not refuse. A speed given directly above the limit for given speeds is [`PhysicsError::SpeedTooHigh`] instead.
    Diverged,
    /// A body description or a setter was given a linear speed above
    /// [`MAX_GIVEN_SPEED`](crate::core::MAX_GIVEN_SPEED) or an angular speed above
    /// [`MAX_GIVEN_SPIN`](crate::core::MAX_GIVEN_SPIN), half of the bounds a step enforces
    /// ([`MAX_SPEED`](crate::core::MAX_SPEED), [`MAX_SPIN`](crate::core::MAX_SPIN)), so that a
    /// body given its speed is not refused by the first step for gravity or a force alone (card
    /// 8-46); a collision can still take it past a bound. Nothing changed.
    SpeedTooHigh,
    /// A [`SpeedPolicy::Clamp`](crate::core::SpeedPolicy::Clamp) limit is not finite, not above
    /// zero, or above [`MAX_GIVEN_SPEED`](crate::core::MAX_GIVEN_SPEED) (linear) or
    /// [`MAX_GIVEN_SPIN`](crate::core::MAX_GIVEN_SPIN) (angular). Nothing changed.
    InvalidSpeedLimit,
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
            Self::InvalidMaterial => {
                f.write_str("friction must be zero or more and restitution from zero to one")
            }
            Self::InvalidSolverConfig => f.write_str("a solver setting is out of range"),
            Self::InvalidSleepConfig => f.write_str("a sleep setting is out of range"),
            Self::JointCapacityExhausted => f.write_str("the world has no free joint slot"),
            Self::JointCapacityTooLarge { max } => {
                write!(f, "joint capacity too large, at most {max} joints allowed")
            }
            Self::StaleJoint => f.write_str("the handle names no live joint of this world"),
            Self::InvalidJoint => f.write_str("a joint setting is out of range"),
            Self::Diverged => f.write_str("the step would give a body a speed beyond the limit"),
            Self::SpeedTooHigh => f.write_str("the speed is beyond the limit"),
            Self::InvalidSpeedLimit => f.write_str("a speed limit is out of range"),
            Self::ManifoldCapacityTooLarge { max } => {
                write!(
                    f,
                    "manifold capacity too large, at most {max} manifolds allowed"
                )
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
            (
                PhysicsError::ManifoldCapacityTooLarge { max: 3 },
                "manifold capacity too large, at most 3 manifolds allowed",
            ),
            (
                PhysicsError::InvalidMaterial,
                "friction must be zero or more and restitution from zero to one",
            ),
            (
                PhysicsError::InvalidSolverConfig,
                "a solver setting is out of range",
            ),
            (
                PhysicsError::InvalidSleepConfig,
                "a sleep setting is out of range",
            ),
            (
                PhysicsError::Diverged,
                "the step would give a body a speed beyond the limit",
            ),
            (PhysicsError::SpeedTooHigh, "the speed is beyond the limit"),
        ];
        check_messages(cases);
    }

    #[test]
    fn the_speed_limit_error_has_its_own_readable_message() {
        check_messages([(
            PhysicsError::InvalidSpeedLimit,
            "a speed limit is out of range",
        )]);
    }

    #[test]
    fn every_joint_error_has_its_own_readable_message() {
        let cases = [
            (
                PhysicsError::JointCapacityExhausted,
                "the world has no free joint slot",
            ),
            (
                PhysicsError::JointCapacityTooLarge { max: 5 },
                "joint capacity too large, at most 5 joints allowed",
            ),
            (
                PhysicsError::StaleJoint,
                "the handle names no live joint of this world",
            ),
            (
                PhysicsError::InvalidJoint,
                "a joint setting is out of range",
            ),
        ];
        check_messages(cases);
    }

    fn check_messages<const N: usize>(cases: [(PhysicsError, &str); N]) {
        for (error, message) in cases {
            assert_eq!(error.to_string(), message);
        }
    }
}
