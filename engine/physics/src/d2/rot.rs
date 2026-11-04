//! 2D orientation as a unit (cos, sin) pair.
//!
//! The pair is advanced in the step by a first-order update followed by a normalisation, so the
//! step calls no `sin`, `cos` or `atan2`. Those are needed only to make a pair from an angle and to
//! read an angle back, and both go through `base::dmath` (the platform function with the feature
//! `cross-platform-determinism` off, the engine's own function with it on).

use crate::core::{HashBits, Quantity, StateHasher};
use base::dmath;

/// A counter-clockwise rotation, stored as `(cos, sin)` of its angle. The fields are private so a
/// value is always unit length (to rounding) or, for a pair made from a non-finite angle, not finite.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rot2 {
    cos: f32,
    sin: f32,
}

impl Rot2 {
    /// No rotation.
    pub const IDENTITY: Self = Self { cos: 1.0, sin: 0.0 };

    /// The rotation by `angle` radians, counter-clockwise. A non-finite angle gives a pair that is
    /// not finite, which world creation refuses.
    #[must_use]
    pub fn from_angle(angle: f32) -> Self {
        let (sin, cos) = dmath::sin_cos(angle);
        Self { cos, sin }
    }

    /// The rotation whose direction is `(cos, sin)`, normalised; `None` when the pair is not finite
    /// or has no length.
    #[must_use]
    pub fn from_cos_sin(cos: f32, sin: f32) -> Option<Self> {
        let length = (cos * cos + sin * sin).sqrt();
        if length.is_finite() && length > 0.0 {
            Some(Self {
                cos: cos / length,
                sin: sin / length,
            })
        } else {
            None
        }
    }

    /// The direction as `(cos, sin)` of the angle.
    #[must_use]
    pub const fn direction(self) -> (f32, f32) {
        (self.cos, self.sin)
    }

    /// The angle in radians, in `(-pi, pi]`.
    #[must_use]
    pub fn angle(self) -> f32 {
        dmath::atan2(self.sin, self.cos)
    }

    /// Turned by the small angle `delta` (radians): first-order update, then normalised. The pair
    /// turns by `atan(delta)`, which is `delta` to second order.
    #[must_use]
    pub fn advanced(self, delta: f32) -> Self {
        let cos = self.cos - self.sin * delta;
        let sin = self.sin + self.cos * delta;
        // The length is at least 1 for a unit start. When the squares overflow (a huge `delta`) or
        // `delta` is not finite, the length is not finite: the result is a pair that is not finite,
        // which `World::step` refuses without changing anything. (A zero length divides to NaN.)
        let length = (cos * cos + sin * sin).sqrt();
        if length.is_finite() {
            Self {
                cos: cos / length,
                sin: sin / length,
            }
        } else {
            Self {
                cos: f32::NAN,
                sin: f32::NAN,
            }
        }
    }
}

impl Default for Rot2 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl HashBits for Rot2 {
    fn write_bits(&self, hasher: &mut StateHasher) {
        hasher.write_f32(self.cos);
        hasher.write_f32(self.sin);
    }
}

impl Quantity for Rot2 {
    fn is_finite(&self) -> bool {
        self.cos.is_finite() && self.sin.is_finite()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, PI};

    const TOLERANCE: f32 = 1e-6;

    #[test]
    fn the_identity_is_cos_one_sin_zero_and_the_default() {
        assert_eq!(Rot2::IDENTITY.direction(), (1.0, 0.0));
        assert_eq!(Rot2::default(), Rot2::IDENTITY);
        assert!(Rot2::IDENTITY.angle().abs() < f32::EPSILON);
    }

    #[test]
    fn from_angle_makes_the_cos_sin_pair() {
        let quarter = Rot2::from_angle(FRAC_PI_2);
        let (cos, sin) = quarter.direction();
        assert!(cos.abs() < TOLERANCE);
        assert!((sin - 1.0).abs() < TOLERANCE);
    }

    #[test]
    fn angle_reads_back_what_from_angle_made() {
        for angle in [-3.0_f32, -1.0, -0.25, 0.0, 0.5, 2.0, 3.0] {
            assert!((Rot2::from_angle(angle).angle() - angle).abs() < 2e-6);
        }
        assert!((Rot2::from_angle(PI + 0.5).angle() - (0.5 - PI)).abs() < 1e-5);
    }

    #[test]
    fn from_cos_sin_normalises() {
        let rot = Rot2::from_cos_sin(3.0, 4.0).map(Rot2::direction);
        assert_eq!(rot, Some((0.6, 0.8)));
    }

    #[test]
    fn from_cos_sin_refuses_a_pair_without_direction() {
        assert_eq!(Rot2::from_cos_sin(0.0, 0.0), None);
        assert_eq!(Rot2::from_cos_sin(f32::NAN, 1.0), None);
        assert_eq!(Rot2::from_cos_sin(1.0, f32::INFINITY), None);
        assert_eq!(Rot2::from_cos_sin(f32::MAX, f32::MAX), None);
        assert_eq!(Rot2::from_cos_sin(1e-30, 1e-30), None);
    }

    #[test]
    fn a_non_finite_angle_gives_a_pair_that_is_not_finite() {
        assert!(!Rot2::from_angle(f32::NAN).is_finite());
        assert!(!Rot2::from_angle(f32::INFINITY).is_finite());
        assert!(Rot2::from_angle(1.0).is_finite());
        let half_bad = Rot2 {
            cos: 1.0,
            sin: f32::NAN,
        };
        assert!(!half_bad.is_finite());
    }

    #[test]
    fn advanced_turns_by_atan_of_the_step_and_stays_unit_length() {
        let delta = 0.1_f32;
        let turned = Rot2::advanced(Rot2::IDENTITY, delta);
        assert!((turned.angle() - dmath::atan2(delta, 1.0)).abs() < TOLERANCE);
        let (cos, sin) = turned.direction();
        assert!((cos * cos + sin * sin - 1.0).abs() < TOLERANCE);
    }

    #[test]
    fn a_turn_that_overflows_or_is_not_finite_gives_a_pair_that_is_not_finite() {
        assert!(!Rot2::IDENTITY.advanced(1e30).is_finite());
        assert!(!Rot2::IDENTITY.advanced(f32::NAN).is_finite());
        assert!(!Rot2::IDENTITY.advanced(f32::INFINITY).is_finite());
        assert!(Rot2::IDENTITY.advanced(1e18).is_finite());
        let empty = Rot2 { cos: 0.0, sin: 0.0 };
        assert!(!empty.advanced(1.0).is_finite());
    }

    #[test]
    fn advanced_by_zero_changes_nothing_and_by_a_negative_turns_back() {
        let start = Rot2::from_angle(0.7);
        assert_eq!(start.advanced(0.0), start);
        let back = start.advanced(-0.05);
        assert!(back.angle() < start.angle());
    }

    #[test]
    fn the_pair_is_hashed_cos_then_sin() {
        let rot = Rot2::from_angle(0.7);
        let mut through = StateHasher::new();
        rot.write_bits(&mut through);
        let mut expected = StateHasher::new();
        let (cos, sin) = rot.direction();
        expected.write_f32(cos);
        expected.write_f32(sin);
        assert_eq!(through.finish(), expected.finish());
    }
}
