//! The 2D joints of v0 (card 4-16): a revolute joint (a hinge, with optional angle limits and an
//! optional motor) and a distance joint (a rod, or a spring with optional length limits).
//!
//! Anchors are given in the frame of each body, relative to its centre of mass (the position of the
//! body), so a joint can be described without knowing where the bodies are. The solver treats a
//! joint as a soft constraint in the same sub-stepping scheme as a contact (see
//! `d2::solver::joint`).
//!
//! A joint is immutable after `create_joint`: there are no setters; to change one, destroy it and
//! create another. The angle limits of a revolute joint lie inside minus pi to pi, so a range that
//! crosses the half turn cannot be written (the same restriction as `Box2D` v3 [K]); turn the
//! `reference_angle` instead. A rope or a strut is a distance joint with a spring of `hertz` 0
//! (no spring force, the rod is slack between its limits) and length limits.
//!
//! What v0 leaves out on purpose: a revolute spring, a distance motor, joint breaking, joint
//! reaction forces, and prismatic, weld and wheel joints.

use crate::core::{JointShape, PhysicsError};
use base::math::Vec2;
use std::f32::consts::PI;

/// The angle range of a revolute joint, in radians, relative to the reference angle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AngleLimits {
    /// The smallest joint angle, from minus pi up to `upper`.
    pub lower: f32,
    /// The largest joint angle, up to pi.
    pub upper: f32,
}

/// A motor that turns the revolute joint toward a speed with a bounded torque.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motor {
    /// The most torque, in newton metres, the motor may apply, zero or more.
    pub max_torque: f32,
    /// The target relative angular speed of the second body against the first, in radians per
    /// second.
    pub speed: f32,
}

/// A hinge: one point of each body is held together.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RevoluteJoint2 {
    /// The hinge point in the frame of the first body, relative to its centre.
    pub anchor_a: Vec2,
    /// The hinge point in the frame of the second body, relative to its centre.
    pub anchor_b: Vec2,
    /// The relative angle of the bodies (second minus first) that counts as joint angle zero, from
    /// minus pi to pi.
    pub reference_angle: f32,
    /// The angle range, or `None` for a hinge that turns freely.
    pub limits: Option<AngleLimits>,
    /// The motor, or `None`.
    pub motor: Option<Motor>,
}

impl RevoluteJoint2 {
    /// A free hinge at the given anchors.
    #[must_use]
    pub const fn new(anchor_a: Vec2, anchor_b: Vec2) -> Self {
        Self {
            anchor_a,
            anchor_b,
            reference_angle: 0.0,
            limits: None,
            motor: None,
        }
    }
}

/// A spring along the rod of a distance joint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    /// Stiffness in hertz, zero or more. Zero means no spring force: the rod is slack between its
    /// limits (a rope).
    pub hertz: f32,
    /// Damping ratio, zero or more.
    pub damping_ratio: f32,
}

/// The length range of a distance joint with a spring, in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LengthLimits {
    /// The shortest length, zero or more.
    pub min: f32,
    /// The longest length, at least `min`.
    pub max: f32,
}

/// A rod between two points, rigid or springy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DistanceJoint2 {
    /// The first end in the frame of the first body, relative to its centre.
    pub anchor_a: Vec2,
    /// The second end in the frame of the second body, relative to its centre.
    pub anchor_b: Vec2,
    /// The rest length in metres, above zero.
    pub length: f32,
    /// The spring. `None` makes the rod rigid, softened only by the joint stiffness of the solver.
    pub spring: Option<Spring>,
    /// The stops, allowed only with a spring and around the rest length (`min <= length <= max`).
    pub limits: Option<LengthLimits>,
}

impl DistanceJoint2 {
    /// A rigid rod of `length` between the anchors.
    #[must_use]
    pub const fn new(anchor_a: Vec2, anchor_b: Vec2, length: f32) -> Self {
        Self {
            anchor_a,
            anchor_b,
            length,
            spring: None,
            limits: None,
        }
    }
}

/// A 2D joint.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Joint2 {
    /// A hinge.
    Revolute(RevoluteJoint2),
    /// A rod or a spring.
    Distance(DistanceJoint2),
}

fn finite(values: &[f32]) -> Result<(), PhysicsError> {
    if values.iter().all(|value| value.is_finite()) {
        Ok(())
    } else {
        Err(PhysicsError::NotFinite)
    }
}

fn require(condition: bool) -> Result<(), PhysicsError> {
    if condition {
        Ok(())
    } else {
        Err(PhysicsError::InvalidJoint)
    }
}

fn within_half_turn(angle: f32) -> bool {
    (-PI..=PI).contains(&angle)
}

impl RevoluteJoint2 {
    fn validate(&self) -> Result<(), PhysicsError> {
        finite(&[
            self.anchor_a.x,
            self.anchor_a.y,
            self.anchor_b.x,
            self.anchor_b.y,
            self.reference_angle,
        ])?;
        require(within_half_turn(self.reference_angle))?;
        if let Some(limits) = self.limits {
            finite(&[limits.lower, limits.upper])?;
            require(
                within_half_turn(limits.lower)
                    && within_half_turn(limits.upper)
                    && limits.lower <= limits.upper,
            )?;
        }
        if let Some(motor) = self.motor {
            finite(&[motor.max_torque, motor.speed])?;
            require(motor.max_torque >= 0.0)?;
        }
        Ok(())
    }
}

impl DistanceJoint2 {
    fn validate(&self) -> Result<(), PhysicsError> {
        finite(&[
            self.anchor_a.x,
            self.anchor_a.y,
            self.anchor_b.x,
            self.anchor_b.y,
            self.length,
        ])?;
        require(self.length > 0.0)?;
        if let Some(spring) = self.spring {
            finite(&[spring.hertz, spring.damping_ratio])?;
            require(spring.hertz >= 0.0 && spring.damping_ratio >= 0.0)?;
        }
        if let Some(limits) = self.limits {
            finite(&[limits.min, limits.max])?;
            require(self.spring.is_some())?;
            require(limits.min >= 0.0 && limits.min <= self.length && self.length <= limits.max)?;
        }
        Ok(())
    }
}

impl JointShape for Joint2 {
    fn validate(&self) -> Result<(), PhysicsError> {
        match self {
            Self::Revolute(joint) => joint.validate(),
            Self::Distance(joint) => joint.validate(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hinge() -> RevoluteJoint2 {
        RevoluteJoint2::new(Vec2::new(0.5, 0.0), Vec2::new(-0.5, 0.0))
    }

    fn rod() -> DistanceJoint2 {
        DistanceJoint2::new(Vec2::ZERO, Vec2::ZERO, 2.0)
    }

    fn check(joint: Joint2) -> Result<(), PhysicsError> {
        joint.validate()
    }

    #[test]
    fn a_plain_hinge_and_a_plain_rod_are_valid() {
        assert_eq!(check(Joint2::Revolute(hinge())), Ok(()));
        assert_eq!(check(Joint2::Distance(rod())), Ok(()));
    }

    #[test]
    fn a_hinge_refuses_numbers_that_are_not_finite() {
        let mut anchor = hinge();
        anchor.anchor_b = Vec2::new(f32::NAN, 0.0);
        let mut reference = hinge();
        reference.reference_angle = f32::INFINITY;
        let mut limits = hinge();
        limits.limits = Some(AngleLimits {
            lower: f32::NAN,
            upper: 0.0,
        });
        let mut motor = hinge();
        motor.motor = Some(Motor {
            max_torque: 1.0,
            speed: f32::NAN,
        });
        for joint in [anchor, reference, limits, motor] {
            assert_eq!(check(Joint2::Revolute(joint)), Err(PhysicsError::NotFinite));
        }
    }

    #[test]
    fn hinge_limits_must_be_ordered_and_inside_half_a_turn() {
        let with = |lower, upper| {
            let mut joint = hinge();
            joint.limits = Some(AngleLimits { lower, upper });
            check(Joint2::Revolute(joint))
        };
        assert_eq!(with(-1.0, 1.0), Ok(()));
        assert_eq!(with(0.5, 0.5), Ok(()));
        assert_eq!(with(-PI, PI), Ok(()));
        assert_eq!(with(1.0, -1.0), Err(PhysicsError::InvalidJoint));
        assert_eq!(with(-3.2, 0.0), Err(PhysicsError::InvalidJoint));
        assert_eq!(with(0.0, 3.2), Err(PhysicsError::InvalidJoint));
    }

    #[test]
    fn the_reference_angle_and_the_motor_torque_have_ranges() {
        let mut turned = hinge();
        turned.reference_angle = 3.2;
        assert_eq!(
            check(Joint2::Revolute(turned)),
            Err(PhysicsError::InvalidJoint)
        );
        turned.reference_angle = PI;
        assert_eq!(check(Joint2::Revolute(turned)), Ok(()));
        let mut motor = hinge();
        motor.motor = Some(Motor {
            max_torque: -1.0,
            speed: 1.0,
        });
        assert_eq!(
            check(Joint2::Revolute(motor)),
            Err(PhysicsError::InvalidJoint)
        );
        motor.motor = Some(Motor {
            max_torque: 0.0,
            speed: -1.0,
        });
        assert_eq!(check(Joint2::Revolute(motor)), Ok(()));
    }

    #[test]
    fn a_rod_needs_a_length_above_zero_and_finite_numbers() {
        for length in [0.0, -1.0] {
            let mut joint = rod();
            joint.length = length;
            assert_eq!(
                check(Joint2::Distance(joint)),
                Err(PhysicsError::InvalidJoint)
            );
        }
        let mut joint = rod();
        joint.length = f32::NAN;
        assert_eq!(check(Joint2::Distance(joint)), Err(PhysicsError::NotFinite));
        let mut joint = rod();
        joint.anchor_a = Vec2::new(0.0, f32::INFINITY);
        assert_eq!(check(Joint2::Distance(joint)), Err(PhysicsError::NotFinite));
    }

    #[test]
    fn a_spring_has_non_negative_numbers() {
        let with = |hertz, damping_ratio| {
            let mut joint = rod();
            joint.spring = Some(Spring {
                hertz,
                damping_ratio,
            });
            check(Joint2::Distance(joint))
        };
        assert_eq!(with(0.0, 0.0), Ok(()));
        assert_eq!(with(3.0, 0.5), Ok(()));
        assert_eq!(with(-1.0, 0.5), Err(PhysicsError::InvalidJoint));
        assert_eq!(with(1.0, -0.5), Err(PhysicsError::InvalidJoint));
        assert_eq!(with(f32::NAN, 0.5), Err(PhysicsError::NotFinite));
    }

    #[test]
    fn length_limits_need_a_spring_and_surround_the_rest_length() {
        let with = |spring: bool, min, max| {
            let mut joint = rod();
            joint.spring = spring.then_some(Spring {
                hertz: 0.0,
                damping_ratio: 0.0,
            });
            joint.limits = Some(LengthLimits { min, max });
            check(Joint2::Distance(joint))
        };
        assert_eq!(with(true, 1.0, 3.0), Ok(()));
        assert_eq!(with(true, 2.0, 2.0), Ok(()));
        assert_eq!(with(true, 0.0, 2.0), Ok(()));
        assert_eq!(with(false, 1.0, 3.0), Err(PhysicsError::InvalidJoint));
        assert_eq!(with(true, 2.5, 3.0), Err(PhysicsError::InvalidJoint));
        assert_eq!(with(true, 1.0, 1.5), Err(PhysicsError::InvalidJoint));
        assert_eq!(with(true, -0.5, 3.0), Err(PhysicsError::InvalidJoint));
        assert_eq!(with(true, f32::NAN, 3.0), Err(PhysicsError::NotFinite));
    }
}
