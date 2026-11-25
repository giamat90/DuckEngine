//! What a step does with a speed beyond the bounds (card 8-50).

use super::{MAX_GIVEN_SPEED, MAX_GIVEN_SPIN, PhysicsError};

/// What `World::step` does with the speed of a body at the end of a step.
///
/// The default is [`SpeedPolicy::Refuse`], the rule of cards 8-25 and 8-36: a step that would leave
/// a body beyond the bounds ([`MAX_SPEED`](super::MAX_SPEED), [`MAX_SPIN`](super::MAX_SPIN)) is
/// refused as a whole ([`PhysicsError::Diverged`]) so the caller sees the blow-up.
/// [`SpeedPolicy::Clamp`] is the choice of other engines (`Box2D`, `Jolt` and `PhysX` have a
/// maximum speed that they clamp to): the step brings every dynamic body that ended faster than
/// the limits you set back to them, keeping the direction, and goes on. The
/// count of bodies it touched is [`World::clamped_bodies`](super::World::clamped_bodies).
///
/// The clamp is applied to the result of the step, so a body that crossed a limit during the step
/// has moved that step with the higher speed, and the contacts of that step were solved with it.
/// It does not hide a value that is not a number or an overflow: those still refuse the step
/// ([`PhysicsError::NotFinite`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub enum SpeedPolicy {
    /// Refuse a step that would leave a body beyond the bounds, as [`PhysicsError::Diverged`].
    #[default]
    Refuse,
    /// Clamp every dynamic body to `linear` metres per second and `angular` radians per second.
    /// Both limits are above zero and at most [`MAX_GIVEN_SPEED`] and [`MAX_GIVEN_SPIN`], the
    /// speeds a body may be given, so a clamped speed is always inside the bounds.
    Clamp {
        /// The fastest linear speed, metres per second.
        linear: f32,
        /// The fastest angular speed, radians per second.
        angular: f32,
    },
}

impl SpeedPolicy {
    /// Checks the limits of [`SpeedPolicy::Clamp`]; [`SpeedPolicy::Refuse`] is always valid.
    ///
    /// # Errors
    /// [`PhysicsError::InvalidSpeedLimit`] when a limit is not finite, not above zero, or above
    /// [`MAX_GIVEN_SPEED`] (linear) or [`MAX_GIVEN_SPIN`] (angular).
    pub fn validate(&self) -> Result<(), PhysicsError> {
        match *self {
            Self::Refuse => Ok(()),
            Self::Clamp { linear, angular }
                if linear > 0.0
                    && linear <= MAX_GIVEN_SPEED
                    && angular > 0.0
                    && angular <= MAX_GIVEN_SPIN =>
            {
                Ok(())
            }
            Self::Clamp { .. } => Err(PhysicsError::InvalidSpeedLimit),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuse_is_the_default_and_is_valid() {
        assert_eq!(SpeedPolicy::default(), SpeedPolicy::Refuse);
        assert_eq!(SpeedPolicy::Refuse.validate(), Ok(()));
    }

    #[test]
    fn a_limit_is_valid_above_zero_up_to_the_given_ceiling() {
        let clamp = |linear, angular| SpeedPolicy::Clamp { linear, angular }.validate();
        assert_eq!(clamp(1.0, 1.0), Ok(()));
        assert_eq!(clamp(MAX_GIVEN_SPEED, MAX_GIVEN_SPIN), Ok(()));
        let bad = Err(PhysicsError::InvalidSpeedLimit);
        assert_eq!(clamp(0.0, 1.0), bad);
        assert_eq!(clamp(1.0, 0.0), bad);
        assert_eq!(clamp(-1.0, 1.0), bad);
        assert_eq!(clamp(1.0, -1.0), bad);
        assert_eq!(clamp(f32::NAN, 1.0), bad);
        assert_eq!(clamp(1.0, f32::NAN), bad);
        assert_eq!(clamp(f32::INFINITY, 1.0), bad);
        assert_eq!(clamp(MAX_GIVEN_SPEED * 1.001, 1.0), bad);
        assert_eq!(clamp(1.0, MAX_GIVEN_SPIN * 1.001), bad);
    }
}
