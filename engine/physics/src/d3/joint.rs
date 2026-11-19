//! The 3D joints of v0 (card 8-7): a ball-and-socket joint, a hinge (with optional angle limits)
//! and a distance joint (a rod, or a rope between two lengths).
//!
//! Anchors and axes are given in the frame of each body, relative to its centre of mass (the
//! position of the body), so a joint can be described without knowing where the bodies are. The
//! solver treats a joint as a soft constraint in the same sub-stepping scheme as a contact (see
//! `d3::solver::joint`).
//!
//! A joint is immutable after `create_joint`: there are no setters; to change one, destroy it and
//! create another. The angle limits of a hinge lie inside minus pi to pi, so a range that crosses
//! the half turn cannot be written (as in 2D); turn the `reference_angle` instead.
//!
//! The angle of a hinge is measured between two frames, one in each body. A frame is the hinge axis
//! and a reference direction perpendicular to it. The first body's reference direction is the first
//! tangent of `axis_a` (the same frame the contacts use, a function of the axis alone); the second
//! body's is that direction carried to `axis_b` by the smallest rotation that turns `axis_a` into
//! `axis_b`, then turned about `axis_b` by minus `reference_angle`. So the angle is zero when the two
//! frames coincide: with equal local axes that is when the bodies have the same orientation, and the
//! angle is the relative angle of the bodies minus the reference angle, as in 2D; with different
//! local axes it is the pose in which the two axes are the same line and the frames agree. The angle
//! is positive when the second body turns counter-clockwise about the first axis against the first.
//! A second axis that points against the first (a negative dot product of the two local axes) is the
//! same line seen from the other side: the joint keeps the first axis on the negative of the second,
//! the angle is still measured about the first axis, and the zero pose is the one in which the two
//! frames agree. The choice is made once, from the local axes, so it cannot flip while the bodies
//! move.
//!
//! A rope (a distance joint with limits) has no direction when its two anchors are on top of each
//! other, so its shortest length cannot act there; a rod or a rope longer than a few millimetres is
//! not affected. A violated limit is corrected at a bounded speed (at most 0.02 rad or 0.02 m of
//! error is counted), so a body that starts far outside its range comes back without being thrown
//! through it. Without the bound a body that is small for its arm (a 0.1 m cube hinged 3 m from its
//! centre) was sent back at a speed that made the step diverge.
//!
//! Known limits (measured in card 8-7, not solved):
//! - Bodies of very different mass joined at points far from their centres can make the soft step
//!   diverge, as in 2D. A rod with a 100 times heavier end holds its length within 0.02 m; 10 000
//!   times heavier is 2 m off; a chain of five balls with a 100 times heavier end is 0.3 m apart at
//!   a link; up to about 30 times is fine.
//! - A body that is small for its arm: the mass times the arm squared over the inertia about the
//!   hinge axis (1350 for a 0.1 m cube on a 3 m arm). Above about 1000 a hinge limit that is
//!   violated far enough can leave the body moving through its range instead of resting at the
//!   near limit; above about 5000, with the body also moving fast into the limit, the step can
//!   diverge. A 0.1 m cube on a 10 m arm under gravity ended non-finite in one of 216 scanned scenes.
//! - A step that diverges used to end with huge but finite values and `Ok`. Since card 8-25
//!   `World::step` refuses a step that would leave a body faster than `MAX_SPEED` or `MAX_SPIN`
//!   (`PhysicsError::Diverged`, the forces of the step are dropped and nothing else changes). A divergence that stays below those bounds is
//!   still not seen: the bound catches the blow-up, not a body that is merely wrong.
//! - A body that turns faster than a step can follow can pass through a limit (no continuous check).
//!
//! What v0 leaves out on purpose (card 8-20 and later): a hinge motor and spring, a spring on the
//! distance joint, joint breaking, joint reaction forces, and prismatic, cone-twist and weld joints.

use crate::core::{JointShape, PhysicsError, Quantity};
use base::math::Vec3;
use std::f32::consts::PI;

/// The largest anchor coordinate or length a joint accepts, in metres. Anything bigger is not a joint
/// of a world this engine can hold (the step would fling the body out of the float range).
const MAX_EXTENT: f32 = 1.0e6;

/// How far the length of a hinge axis may be from one before it is refused (the square of the
/// length is compared).
const AXIS_TOLERANCE: f32 = 1.0e-3;

/// The angle range of a hinge, in radians, relative to the reference angle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AngleLimits {
    /// The smallest joint angle, from minus pi up to `upper`.
    pub lower: f32,
    /// The largest joint angle, up to pi.
    pub upper: f32,
}

/// A ball-and-socket joint: one point of each body is held together; the bodies turn freely about
/// it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BallJoint3 {
    /// The joint point in the frame of the first body, relative to its centre.
    pub anchor_a: Vec3,
    /// The joint point in the frame of the second body, relative to its centre.
    pub anchor_b: Vec3,
}

impl BallJoint3 {
    /// A ball-and-socket joint at the given anchors.
    #[must_use]
    pub const fn new(anchor_a: Vec3, anchor_b: Vec3) -> Self {
        Self { anchor_a, anchor_b }
    }
}

/// A hinge: one point of each body is held together and one axis of each body is kept parallel, so
/// the second body turns about the axis only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HingeJoint3 {
    /// The hinge point in the frame of the first body, relative to its centre.
    pub anchor_a: Vec3,
    /// The hinge point in the frame of the second body, relative to its centre.
    pub anchor_b: Vec3,
    /// The hinge axis in the frame of the first body, of length one.
    pub axis_a: Vec3,
    /// The hinge axis in the frame of the second body, of length one.
    pub axis_b: Vec3,
    /// The relative angle of the bodies (second against first, about the axis) that counts as joint
    /// angle zero, from minus pi to pi.
    pub reference_angle: f32,
    /// The angle range, or `None` for a hinge that turns freely.
    pub limits: Option<AngleLimits>,
}

impl HingeJoint3 {
    /// A free hinge at the given anchors, about `axis_a` of the first body and `axis_b` of the
    /// second.
    #[must_use]
    pub const fn new(anchor_a: Vec3, anchor_b: Vec3, axis_a: Vec3, axis_b: Vec3) -> Self {
        Self {
            anchor_a,
            anchor_b,
            axis_a,
            axis_b,
            reference_angle: 0.0,
            limits: None,
        }
    }
}

/// The length range of a rope, in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LengthLimits {
    /// The shortest length, zero or more.
    pub min: f32,
    /// The longest length, at least `min`.
    pub max: f32,
}

/// A rod between two points, or a rope between two lengths.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DistanceJoint3 {
    /// The first end in the frame of the first body, relative to its centre.
    pub anchor_a: Vec3,
    /// The second end in the frame of the second body, relative to its centre.
    pub anchor_b: Vec3,
    /// The rest length in metres, above zero. Without limits the rod holds exactly this length.
    pub length: f32,
    /// The stops. With limits the joint has no force of its own: the ends are free between `min`
    /// and `max` and only the stops act (a rope, a strut). They surround the rest length
    /// (`min <= length <= max`).
    pub limits: Option<LengthLimits>,
}

impl DistanceJoint3 {
    /// A rigid rod of `length` between the anchors.
    #[must_use]
    pub const fn new(anchor_a: Vec3, anchor_b: Vec3, length: f32) -> Self {
        Self {
            anchor_a,
            anchor_b,
            length,
            limits: None,
        }
    }
}

/// A 3D joint.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Joint3 {
    /// A ball-and-socket joint.
    Ball(BallJoint3),
    /// A hinge.
    Hinge(HingeJoint3),
    /// A rod or a rope.
    Distance(DistanceJoint3),
}

fn finite(vectors: &[Vec3], numbers: &[f32]) -> Result<(), PhysicsError> {
    let vectors_ok = vectors.iter().all(Quantity::is_finite);
    if vectors_ok && numbers.iter().all(Quantity::is_finite) {
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

fn reasonable(vectors: &[Vec3], numbers: &[f32]) -> Result<(), PhysicsError> {
    let vectors_ok = vectors
        .iter()
        .all(|vector| vector.length_squared() <= MAX_EXTENT * MAX_EXTENT);
    require(vectors_ok && numbers.iter().all(|number| number.abs() <= MAX_EXTENT))
}

fn within_half_turn(angle: f32) -> bool {
    (-PI..=PI).contains(&angle)
}

fn is_unit(axis: Vec3) -> bool {
    (axis.length_squared() - 1.0).abs() <= AXIS_TOLERANCE
}

impl HingeJoint3 {
    fn validate(&self) -> Result<(), PhysicsError> {
        finite(
            &[self.anchor_a, self.anchor_b, self.axis_a, self.axis_b],
            &[self.reference_angle],
        )?;
        require(is_unit(self.axis_a) && is_unit(self.axis_b))?;
        reasonable(&[self.anchor_a, self.anchor_b], &[])?;
        require(within_half_turn(self.reference_angle))?;
        if let Some(limits) = self.limits {
            finite(&[], &[limits.lower, limits.upper])?;
            require(
                within_half_turn(limits.lower)
                    && within_half_turn(limits.upper)
                    && limits.lower <= limits.upper,
            )?;
        }
        Ok(())
    }
}

impl DistanceJoint3 {
    fn validate(&self) -> Result<(), PhysicsError> {
        finite(&[self.anchor_a, self.anchor_b], &[self.length])?;
        require(self.length > 0.0)?;
        reasonable(&[self.anchor_a, self.anchor_b], &[self.length])?;
        if let Some(limits) = self.limits {
            finite(&[], &[limits.min, limits.max])?;
            reasonable(&[], &[limits.min, limits.max])?;
            require(limits.min >= 0.0 && limits.min <= self.length && self.length <= limits.max)?;
        }
        Ok(())
    }
}

impl JointShape for Joint3 {
    fn validate(&self) -> Result<(), PhysicsError> {
        match self {
            Self::Ball(joint) => finite(&[joint.anchor_a, joint.anchor_b], &[])
                .and_then(|()| reasonable(&[joint.anchor_a, joint.anchor_b], &[])),
            Self::Hinge(joint) => joint.validate(),
            Self::Distance(joint) => joint.validate(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const X: Vec3 = Vec3::new(1.0, 0.0, 0.0);
    const NOT_A_NUMBER: f32 = f32::NAN;
    const BAD: Vec3 = Vec3::new(NOT_A_NUMBER, 0.0, 0.0);

    fn hinge() -> HingeJoint3 {
        HingeJoint3::new(Vec3::ZERO, Vec3::ZERO, X, X)
    }

    fn rod() -> DistanceJoint3 {
        DistanceJoint3::new(Vec3::ZERO, Vec3::ZERO, 2.0)
    }

    /// True when every joint in `joints` is refused with `error`.
    fn all_refused(joints: &[Joint3], error: PhysicsError) -> bool {
        joints.iter().all(|joint| joint.validate() == Err(error))
    }

    fn hinges(variants: &[HingeJoint3]) -> Vec<Joint3> {
        variants.iter().copied().map(Joint3::Hinge).collect()
    }

    fn rods(variants: &[DistanceJoint3]) -> Vec<Joint3> {
        variants.iter().copied().map(Joint3::Distance).collect()
    }

    fn range(lower: f32, upper: f32) -> AngleLimits {
        AngleLimits { lower, upper }
    }

    fn rope(min: f32, max: f32) -> DistanceJoint3 {
        DistanceJoint3 {
            limits: Some(LengthLimits { min, max }),
            ..rod()
        }
    }

    #[test]
    fn plain_joints_are_valid() {
        assert_eq!(Joint3::Ball(BallJoint3::new(X, -X)).validate(), Ok(()));
        assert_eq!(Joint3::Hinge(hinge()).validate(), Ok(()));
        assert_eq!(Joint3::Distance(rod()).validate(), Ok(()));
    }

    #[test]
    fn a_ball_joint_anchor_that_is_not_finite_is_refused() {
        let joints = [
            Joint3::Ball(BallJoint3::new(BAD, Vec3::ZERO)),
            Joint3::Ball(BallJoint3::new(Vec3::ZERO, BAD)),
        ];
        assert!(all_refused(&joints, PhysicsError::NotFinite));
    }

    #[test]
    fn a_hinge_number_that_is_not_finite_is_refused_in_every_field() {
        let base = hinge();
        let joints = hinges(&[
            HingeJoint3 {
                anchor_a: BAD,
                ..base
            },
            HingeJoint3 {
                anchor_b: BAD,
                ..base
            },
            HingeJoint3 {
                axis_a: BAD,
                ..base
            },
            HingeJoint3 {
                axis_b: BAD,
                ..base
            },
            HingeJoint3 {
                reference_angle: NOT_A_NUMBER,
                ..base
            },
            HingeJoint3 {
                limits: Some(range(NOT_A_NUMBER, 0.5)),
                ..base
            },
            HingeJoint3 {
                limits: Some(range(-0.5, NOT_A_NUMBER)),
                ..base
            },
        ]);
        assert!(all_refused(&joints, PhysicsError::NotFinite));
    }

    #[test]
    fn a_distance_joint_number_that_is_not_finite_is_refused_in_every_field() {
        let base = rod();
        let joints = rods(&[
            DistanceJoint3 {
                anchor_a: BAD,
                ..base
            },
            DistanceJoint3 {
                anchor_b: BAD,
                ..base
            },
            DistanceJoint3 {
                length: NOT_A_NUMBER,
                ..base
            },
            rope(NOT_A_NUMBER, 3.0),
            rope(1.0, NOT_A_NUMBER),
        ]);
        assert!(all_refused(&joints, PhysicsError::NotFinite));
    }

    #[test]
    fn a_hinge_axis_must_have_length_one() {
        let base = hinge();
        let wrong = [
            Vec3::ZERO,
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
        ];
        let first: Vec<_> = wrong
            .iter()
            .map(|&axis| HingeJoint3 {
                axis_a: axis,
                ..base
            })
            .collect();
        let second: Vec<_> = wrong
            .iter()
            .map(|&axis| HingeJoint3 {
                axis_b: axis,
                ..base
            })
            .collect();
        assert!(all_refused(&hinges(&first), PhysicsError::InvalidJoint));
        assert!(all_refused(&hinges(&second), PhysicsError::InvalidJoint));
        let slightly_off = HingeJoint3 {
            axis_a: Vec3::new(1.0001, 0.0, 0.0),
            ..base
        };
        assert_eq!(Joint3::Hinge(slightly_off).validate(), Ok(()));
    }

    #[test]
    fn a_hinge_angle_must_lie_inside_the_half_turn() {
        let base = hinge();
        let outside = hinges(&[
            HingeJoint3 {
                reference_angle: PI + 0.01,
                ..base
            },
            HingeJoint3 {
                reference_angle: -PI - 0.01,
                ..base
            },
        ]);
        assert!(all_refused(&outside, PhysicsError::InvalidJoint));
        let edge = HingeJoint3 {
            reference_angle: PI,
            ..base
        };
        assert_eq!(Joint3::Hinge(edge).validate(), Ok(()));
    }

    #[test]
    fn hinge_limits_must_be_in_order_and_inside_the_half_turn() {
        let base = hinge();
        for (lower, upper) in [(-0.5, 0.5), (-PI, PI), (0.0, 0.0)] {
            let joint = HingeJoint3 {
                limits: Some(range(lower, upper)),
                ..base
            };
            assert_eq!(Joint3::Hinge(joint).validate(), Ok(()));
        }
        let bad = hinges(&[
            HingeJoint3 {
                limits: Some(range(0.5, -0.5)),
                ..base
            },
            HingeJoint3 {
                limits: Some(range(-4.0, 0.0)),
                ..base
            },
            HingeJoint3 {
                limits: Some(range(0.0, 4.0)),
                ..base
            },
        ]);
        assert!(all_refused(&bad, PhysicsError::InvalidJoint));
    }

    #[test]
    fn a_rod_needs_a_positive_length() {
        let base = rod();
        let joints = rods(&[
            DistanceJoint3 {
                length: 0.0,
                ..base
            },
            DistanceJoint3 {
                length: -1.0,
                ..base
            },
        ]);
        assert!(all_refused(&joints, PhysicsError::InvalidJoint));
    }

    #[test]
    fn a_rope_needs_limits_around_the_rest_length() {
        for (min, max) in [(0.0, 2.0), (2.0, 2.0), (1.0, 5.0)] {
            assert_eq!(Joint3::Distance(rope(min, max)).validate(), Ok(()));
        }
        let bad = rods(&[rope(-0.1, 3.0), rope(2.1, 3.0), rope(1.0, 1.9)]);
        assert!(all_refused(&bad, PhysicsError::InvalidJoint));
    }

    #[test]
    fn an_anchor_or_length_that_is_absurdly_large_is_refused() {
        let far = Vec3::new(0.0, 2.0e6, 0.0);
        let joints = [
            Joint3::Ball(BallJoint3::new(far, Vec3::ZERO)),
            Joint3::Ball(BallJoint3::new(Vec3::ZERO, far)),
            Joint3::Hinge(HingeJoint3 {
                anchor_a: far,
                ..hinge()
            }),
            Joint3::Hinge(HingeJoint3 {
                anchor_b: far,
                ..hinge()
            }),
            Joint3::Distance(DistanceJoint3 {
                anchor_a: far,
                ..rod()
            }),
            Joint3::Distance(DistanceJoint3 {
                anchor_b: far,
                ..rod()
            }),
            Joint3::Distance(DistanceJoint3 {
                length: 2.0e6,
                ..rod()
            }),
            Joint3::Distance(rope(1.0, 2.0e6)),
        ];
        assert!(all_refused(&joints, PhysicsError::InvalidJoint));
        let big = Vec3::new(0.0, 9.0e5, 0.0);
        assert_eq!(Joint3::Ball(BallJoint3::new(big, -big)).validate(), Ok(()));
    }
}
