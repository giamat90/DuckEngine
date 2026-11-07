//! The tunables of sleeping (card 4-14): when a resting island of bodies stops costing time.
//!
//! An island is a set of dynamic bodies joined by touching manifolds (see `islands`). Every
//! dynamic body that moves slower than both speed limits for `time_to_sleep` seconds in a row has
//! "rested" that long; an island goes to sleep when every one of its bodies has. The values follow
//! `Box2D` v3 **[K]** (0.5 s, 0.05 m/s) and v2 **[K]** (2 degrees per second); the reasons for the
//! choice made here are in `reviews/4-14.md`.

use super::error::PhysicsError;

/// How long, in seconds, every body of an island must stay below the speed limits before the
/// island sleeps: 0.5, the value of `Box2D` v3 **[K]**. Shorter makes slowly settling stacks sleep
/// before they have settled; longer keeps resting scenes awake (and costly) for no gain.
pub const DEFAULT_TIME_TO_SLEEP: f32 = 0.5;

/// The linear speed, in metres per second, at or below which a body counts as resting: 0.05, the
/// value of `Box2D` v3 **[K]**. A settled stack of the soft solver moves far less than this.
pub const DEFAULT_SLEEP_LINEAR_SPEED: f32 = 0.05;

/// The angular speed, in radians per second, at or below which a body counts as resting: 2 degrees
/// per second (0.0349), the value of `Box2D` v2 **[K]**. A box of half width 0.5 m turning at this
/// rate moves its corners by 1.7 cm/s, under the linear limit.
pub const DEFAULT_SLEEP_ANGULAR_SPEED: f32 = 0.034_906_585;

/// The tunables of sleeping. All are fixed when the world is built.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SleepConfig {
    /// False turns sleeping off: no body ever sleeps and the world steps exactly as a world with
    /// sleeping on does until the first island would have slept.
    pub enabled: bool,
    /// Seconds an island must rest before it sleeps, above zero ([`DEFAULT_TIME_TO_SLEEP`]).
    pub time_to_sleep: f32,
    /// Linear speed limit in metres per second, zero or more ([`DEFAULT_SLEEP_LINEAR_SPEED`]).
    pub linear_speed: f32,
    /// Angular speed limit in radians per second, zero or more ([`DEFAULT_SLEEP_ANGULAR_SPEED`]).
    pub angular_speed: f32,
}

impl SleepConfig {
    /// Sleeping on, with the documented defaults.
    pub const DEFAULT: Self = Self {
        enabled: true,
        time_to_sleep: DEFAULT_TIME_TO_SLEEP,
        linear_speed: DEFAULT_SLEEP_LINEAR_SPEED,
        angular_speed: DEFAULT_SLEEP_ANGULAR_SPEED,
    };

    /// Sleeping off; the other values are the defaults.
    pub const DISABLED: Self = Self {
        enabled: false,
        ..Self::DEFAULT
    };

    /// Checks every field against its range.
    ///
    /// # Errors
    /// [`PhysicsError::InvalidSleepConfig`] when a number is not finite, the time is not above zero
    /// or a speed limit is below zero.
    pub fn validate(&self) -> Result<(), PhysicsError> {
        let valid = self.time_to_sleep.is_finite()
            && self.time_to_sleep > 0.0
            && self.linear_speed.is_finite()
            && self.linear_speed >= 0.0
            && self.angular_speed.is_finite()
            && self.angular_speed >= 0.0;
        if valid {
            Ok(())
        } else {
            Err(PhysicsError::InvalidSleepConfig)
        }
    }
}

impl Default for SleepConfig {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invalid(change: impl FnOnce(&mut SleepConfig)) -> bool {
        let mut config = SleepConfig::DEFAULT;
        change(&mut config);
        config.validate() == Err(PhysicsError::InvalidSleepConfig)
    }

    #[test]
    fn the_defaults_are_valid_documented_values() {
        let config = SleepConfig::default();
        assert_eq!(config, SleepConfig::DEFAULT);
        assert_eq!(config.validate(), Ok(()));
        assert!(config.enabled);
        assert_eq!(config.time_to_sleep.to_bits(), 0.5_f32.to_bits());
        assert_eq!(config.linear_speed.to_bits(), 0.05_f32.to_bits());
        assert!((config.angular_speed - 2.0_f32.to_radians()).abs() < 1e-6);
    }

    #[test]
    fn disabled_differs_from_the_default_only_in_the_flag() {
        let config = SleepConfig::DISABLED;
        assert!(!config.enabled);
        assert_eq!(
            SleepConfig {
                enabled: true,
                ..config
            },
            SleepConfig::DEFAULT
        );
        assert_eq!(config.validate(), Ok(()));
    }

    #[test]
    fn the_time_must_be_finite_and_above_zero() {
        assert!(invalid(|c| c.time_to_sleep = 0.0));
        assert!(invalid(|c| c.time_to_sleep = -1.0));
        assert!(invalid(|c| c.time_to_sleep = f32::NAN));
        assert!(invalid(|c| c.time_to_sleep = f32::INFINITY));
        assert!(!invalid(|c| c.time_to_sleep = 0.001));
    }

    #[test]
    fn the_speed_limits_must_be_finite_and_zero_or_more() {
        assert!(invalid(|c| c.linear_speed = -0.1));
        assert!(invalid(|c| c.linear_speed = f32::NAN));
        assert!(invalid(|c| c.linear_speed = f32::INFINITY));
        assert!(!invalid(|c| c.linear_speed = 0.0));
        assert!(invalid(|c| c.angular_speed = -0.1));
        assert!(invalid(|c| c.angular_speed = f32::NAN));
        assert!(invalid(|c| c.angular_speed = f32::INFINITY));
        assert!(!invalid(|c| c.angular_speed = 0.0));
    }
}
