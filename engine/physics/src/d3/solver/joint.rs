//! Joints in the 3D soft step (card 8-7): the ball-and-socket joint, the hinge and the distance
//! joint as soft constraints in the same sub-stepping scheme as the contacts, as in 2D
//! (`d2::solver::joint`).
//!
//! *Order.* Joints are not coloured and are solved serially: in every pass of a sub-step (warm
//! start, solve, relax) the joints are visited first, one after the other in joint-slot order, and
//! the contacts follow. The result does not depend on a scheduler.
//!
//! *Softness.* The point of a ball joint or a hinge, the alignment of a hinge and a rod are held by
//! a soft constraint of `joint_hertz` and `joint_damping_ratio` (capped at a quarter of the sub-step
//! rate, like the contacts): in the solve pass the error feeds a velocity bias, in the relax pass
//! there is no bias. A one-sided limit (angle, length) is speculative when it is not reached yet and
//! soft when it is violated (`core::soft::JointSweep`).
//!
//! *Math.* Anchors and axes are rotated into the world at the start of the step; within the step the
//! rotation each body made so far (`SolverBody::turn`) is applied to them, and the position error is
//! the relative translation plus the change of the rotated anchors, as for contacts. The point
//! constraint is three rows solved together with the 3 by 3 effective mass. A hinge keeps its two
//! axes parallel with two rows perpendicular to the first axis (the error is the cross product of
//! the axes, which is the rotation about those two directions). The hinge angle is read with `atan2`
//! through the deterministic math facade (once per limited joint and sub-step). Plain `f32`
//! operations in a fixed order: no fused multiply-add, no `sin`, `cos` outside `dmath`.

use super::body::SolverBody;
use super::constraint::{Ends, tangents_of};
use crate::core::soft::{JointSweep, Softness, inverse_or_zero, one_sided};
use crate::core::{Body, HashBits, JointHandle, Quantity, StateHasher};
use crate::d3::{AngleLimits, BallJoint3, D3, DistanceJoint3, HingeJoint3, LengthLimits};
use base::dmath;
use base::math::{Mat3, Vec3};

/// A rod axis shorter than this, in metres, is read as no axis (two anchors on top of each other):
/// the rod then pulls along nothing instead of along a direction made of noise.
const NEGLIGIBLE_LENGTH: f32 = 1e-6;

/// The most a violated limit is corrected at once, in radians for a hinge and metres for a rope. The
/// soft limit turns its error into a speed, so a body that starts far outside the range (or is
/// thrown far past it) would be sent back at a speed that carries it through the whole range;
/// beyond this the error counts as this much and the body comes back at a bounded speed.
const MAX_LIMIT_CORRECTION: f32 = 0.02;

/// Axes this close to opposite are carried by a half turn (see [`carried`]).
const CARRY_LIMIT: f32 = 1e-3;

/// The impulses a joint has accumulated: what it carries from step to step as the warm start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct JointImpulses {
    /// The force that holds the two anchors together (ball joint, hinge).
    pub point: Vec3,
    /// Hinge: the angular impulse that keeps the axes parallel, as a world vector.
    pub align: Vec3,
    /// Rod: along the axis.
    pub rod: f32,
    /// The lower limit (angle or length).
    pub lower: f32,
    /// The upper limit.
    pub upper: f32,
}

impl JointImpulses {
    pub(super) const NONE: Self = Self {
        point: Vec3::ZERO,
        align: Vec3::ZERO,
        rod: 0.0,
        lower: 0.0,
        upper: 0.0,
    };

    pub(super) fn is_finite(&self) -> bool {
        self.point.is_finite()
            && self.align.is_finite()
            && [self.rod, self.lower, self.upper]
                .iter()
                .all(Quantity::is_finite)
    }

    pub(super) fn write_bits(&self, hasher: &mut StateHasher) {
        self.point.write_bits(hasher);
        self.align.write_bits(hasher);
        for value in [self.rod, self.lower, self.upper] {
            hasher.write_f32(value);
        }
    }
}

/// The passes over the joints of a sub-step.
#[derive(Clone, Copy)]
pub(super) enum JointPass<'a> {
    /// Applies the impulses of the last step.
    WarmStart,
    /// One soft solve (the sweep says whether it has the bias).
    Solve(&'a JointSweep),
}

/// The angle of the hinge, in `(-pi, pi]`: from the first body's reference direction to the second's,
/// counter-clockwise about `axis`.
fn hinge_angle(axis: Vec3, first: Vec3, second: Vec3) -> f32 {
    dmath::atan2(first.cross(second).dot(axis), first.dot(second))
}

/// What a hinge needs besides its anchors: the axes and the reference directions at the start of
/// the step, in the world, and the limits.
#[derive(Clone, Copy, Debug)]
struct HingeTerms {
    axis_a: Vec3,
    axis_b: Vec3,
    reference_a: Vec3,
    reference_b: Vec3,
    limits: Option<AngleLimits>,
}

/// What a rod needs besides its anchors.
#[derive(Clone, Copy, Debug)]
struct RodTerms {
    length: f32,
    /// True for a rope: no force of its own, only the limits act.
    slack: bool,
    limits: Option<LengthLimits>,
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    Ball,
    Hinge(HingeTerms),
    Rod(RodTerms),
}

/// What a joint is prepared with.
pub(super) struct JointSetup<'a> {
    pub handle: JointHandle<D3>,
    /// Slot indices of the two bodies.
    pub slots: (usize, usize),
    /// The two bodies as they are at the start of the step.
    pub bodies: [&'a Body<D3>; 2],
}

/// A prepared joint.
#[derive(Clone, Copy, Debug)]
pub(super) struct JointConstraint {
    pub a: usize,
    pub b: usize,
    pub handle: JointHandle<D3>,
    /// True when a body of the joint moves this step; a joint without one is kept as it is.
    pub solved: bool,
    /// Position of the second centre minus the first at the start of the step.
    centres: Vec3,
    /// The anchors rotated into the world at the start of the step, relative to each centre.
    anchor_a: Vec3,
    anchor_b: Vec3,
    kind: Kind,
    pub impulses: JointImpulses,
}

/// `axis` turned by `angle` about the unit vector `about`, where `axis` is perpendicular to it.
fn turned_about(axis: Vec3, about: Vec3, angle: f32) -> Vec3 {
    let (sin, cos) = dmath::sin_cos(angle);
    axis * cos + about.cross(axis) * sin
}

/// `direction`, perpendicular to the unit vector `from`, carried to the plane perpendicular to the
/// unit vector `to` by the smallest rotation that turns `from` into `to`. Equal vectors leave it
/// alone; opposite vectors turn it by a half turn about itself, which leaves it alone too.
fn carried(direction: Vec3, from: Vec3, to: Vec3) -> Vec3 {
    let cosine = from.dot(to);
    if cosine < -1.0 + CARRY_LIMIT {
        return direction;
    }
    let across = from.cross(to);
    let once = across.cross(direction);
    direction + once + across.cross(once) * (1.0 / (1.0 + cosine))
}

/// The unit vector of `axis`, or `axis` itself when it cannot be scaled (the description checked it).
fn unit(axis: Vec3) -> Vec3 {
    axis.normalized().unwrap_or(axis)
}

impl JointConstraint {
    /// A ball-and-socket joint.
    pub(super) fn ball(setup: &JointSetup, joint: &BallJoint3) -> Self {
        Self::with_kind(setup, (joint.anchor_a, joint.anchor_b), Kind::Ball)
    }

    /// A hinge.
    pub(super) fn hinge(setup: &JointSetup, joint: &HingeJoint3) -> Self {
        let [first, second] = setup.bodies;
        let axis_a = unit(joint.axis_a);
        // A second axis that points against the first is the same line seen from the other side:
        // the sign is fixed here, from the two local axes, and the alignment keeps the first axis
        // on `sign x axis_b`. Aligning on the raw cross product would leave the opposite pose as an
        // unstable equilibrium that noise tips over in any rotated frame.
        let sign = if axis_a.dot(unit(joint.axis_b)) < 0.0 {
            -1.0
        } else {
            1.0
        };
        let axis_b = unit(joint.axis_b) * sign;
        let reference_a = tangents_of(axis_a)[0];
        let reference_b = turned_about(
            carried(reference_a, axis_a, axis_b),
            axis_b,
            -joint.reference_angle,
        );
        let terms = HingeTerms {
            axis_a: first.rotation().rotate(axis_a),
            axis_b: second.rotation().rotate(axis_b),
            reference_a: first.rotation().rotate(reference_a),
            reference_b: second.rotation().rotate(reference_b),
            limits: joint.limits,
        };
        Self::with_kind(setup, (joint.anchor_a, joint.anchor_b), Kind::Hinge(terms))
    }

    /// A rod or a rope.
    pub(super) fn rod(setup: &JointSetup, joint: &DistanceJoint3) -> Self {
        let terms = RodTerms {
            length: joint.length,
            slack: joint.limits.is_some(),
            limits: joint.limits,
        };
        Self::with_kind(setup, (joint.anchor_a, joint.anchor_b), Kind::Rod(terms))
    }

    fn with_kind(setup: &JointSetup, local: (Vec3, Vec3), kind: Kind) -> Self {
        let [first, second] = setup.bodies;
        Self {
            a: setup.slots.0,
            b: setup.slots.1,
            handle: setup.handle,
            solved: false,
            centres: second.position() - first.position(),
            anchor_a: first.rotation().rotate(local.0),
            anchor_b: second.rotation().rotate(local.1),
            kind,
            impulses: JointImpulses::NONE,
        }
    }

    /// The anchors as the bodies have turned them so far in the step.
    fn arms(&self, ends: &Ends) -> (Vec3, Vec3) {
        (
            ends.a.turn.rotate(self.anchor_a),
            ends.b.turn.rotate(self.anchor_b),
        )
    }

    /// The vector from the first anchor to the second now.
    fn separation(&self, ends: &Ends, arms: (Vec3, Vec3)) -> Vec3 {
        (ends.b.shift - ends.a.shift) + (arms.1 - arms.0) + self.centres
    }

    /// Applies the impulses of the last step at the start of a sub-step.
    pub(super) fn warm_start(&self, ends: &mut Ends) {
        let arms = self.arms(ends);
        let i = self.impulses;
        match self.kind {
            Kind::Ball => push_at(ends, arms, i.point),
            Kind::Hinge(terms) => {
                let axis = ends.a.turn.rotate(terms.axis_a);
                let held = Held::new(ends, arms, axis);
                let amount = i.lower - i.upper;
                push_spin(ends, i.align + axis * amount);
                push_at(ends, arms, i.point + held.reaction(amount));
            }
            Kind::Rod(_) => {
                let (axis, _) = axis_of(self.separation(ends, arms));
                push_at(ends, arms, axis * (i.rod + i.lower - i.upper));
            }
        }
    }

    /// One soft sweep.
    pub(super) fn solve(&mut self, ends: &mut Ends, sweep: &JointSweep) {
        match self.kind {
            Kind::Ball => self.solve_point(ends, sweep),
            Kind::Hinge(terms) => {
                self.solve_limits(&terms, ends, sweep);
                self.solve_hinge_rows(&terms, ends, sweep);
            }
            Kind::Rod(terms) => self.solve_rod(&terms, ends, sweep),
        }
    }

    /// The two angle stops of a hinge, about the first body's axis.
    fn solve_limits(&mut self, terms: &HingeTerms, ends: &mut Ends, sweep: &JointSweep) {
        let Some(limits) = terms.limits else {
            return;
        };
        let axis = ends.a.turn.rotate(terms.axis_a);
        let angle = hinge_angle(
            axis,
            ends.a.turn.rotate(terms.reference_a),
            ends.b.turn.rotate(terms.reference_b),
        );
        let arms = self.arms(ends);
        let held = Held::new(ends, arms, axis);
        let mass = held.mass(ends);
        let spin = axis.dot(ends.b.spin - ends.a.spin);
        let applied = one_sided(
            &mut self.impulses.lower,
            (
                mass,
                (angle - limits.lower).max(-MAX_LIMIT_CORRECTION),
                spin,
            ),
            sweep,
        );
        turn_held(ends, arms, &held, applied);
        let spin = axis.dot(ends.a.spin - ends.b.spin);
        let applied = one_sided(
            &mut self.impulses.upper,
            (
                mass,
                (limits.upper - angle).max(-MAX_LIMIT_CORRECTION),
                spin,
            ),
            sweep,
        );
        turn_held(ends, arms, &held, -applied);
    }

    /// Holds the anchors together and the two axes parallel: three rows of point and two rows of
    /// alignment, solved together. Solved one after the other the rows chase each other (the arm
    /// from the anchor to the centre turns a small error of the axes into a large motion of the
    /// point), which lets a hinge with a heavy body wobble.
    fn solve_hinge_rows(&mut self, terms: &HingeTerms, ends: &mut Ends, sweep: &JointSweep) {
        let arms = self.arms(ends);
        let (ra, rb) = arms;
        let axis_a = ends.a.turn.rotate(terms.axis_a);
        let axis_b = ends.b.turn.rotate(terms.axis_b);
        let rows = tangents_of(axis_a);
        let (bias, mass_scale, impulse_scale) = soft_terms(sweep);
        let speed = ends.b.velocity_at(rb) - ends.a.velocity_at(ra);
        let relative = ends.b.spin - ends.a.spin;
        let error = if sweep.use_bias {
            self.separation(ends, arms)
        } else {
            Vec3::ZERO
        };
        let tilt = axis_a.cross(axis_b);
        let [u, v] = rows;
        let coupling = rows.map(|row| coupling_of(ends, arms, row));
        let inertia = |first: Vec3, second: Vec3| first.dot(inertia_times(ends, second));
        let (point, align) = block_solve(
            point_mass(ends, arms).inverse(),
            coupling,
            [
                [inertia(u, u), inertia(u, v)],
                [inertia(v, u), inertia(v, v)],
            ],
            (
                speed + error * bias,
                rows.map(|row| row.dot(relative) + bias * row.dot(tilt)),
            ),
        );
        let impulse = -(point * mass_scale) - self.impulses.point * impulse_scale;
        let torque = rows
            .iter()
            .zip(align)
            .map(|(row, solved)| {
                *row * (-(mass_scale * solved) - impulse_scale * row.dot(self.impulses.align))
            })
            .fold(Vec3::ZERO, |sum, one| sum + one);
        self.impulses.point += impulse;
        self.impulses.align += torque;
        push_at(ends, arms, impulse);
        push_spin(ends, torque);
    }

    /// Holds the two anchors together.
    fn solve_point(&mut self, ends: &mut Ends, sweep: &JointSweep) {
        let arms = self.arms(ends);
        let (ra, rb) = arms;
        let speed = ends.b.velocity_at(rb) - ends.a.velocity_at(ra);
        let (bias, mass_scale, impulse_scale) = soft_terms(sweep);
        let error = if sweep.use_bias {
            self.separation(ends, arms)
        } else {
            Vec3::ZERO
        };
        let wanted = point_mass(ends, arms)
            .inverse()
            .map_or(Vec3::ZERO, |inverse| inverse * (speed + error * bias));
        let impulse = -(wanted * mass_scale) - self.impulses.point * impulse_scale;
        self.impulses.point += impulse;
        push_at(ends, arms, impulse);
    }

    fn solve_rod(&mut self, terms: &RodTerms, ends: &mut Ends, sweep: &JointSweep) {
        let arms = self.arms(ends);
        let (axis, length) = axis_of(self.separation(ends, arms));
        if axis == Vec3::ZERO && length.is_finite() {
            // Anchors on top of each other: there is no direction to pull along, and no impulse
            // may pile up for a direction that does not exist. A NaN length is not this case: it
            // must reach the finiteness check of the step.
            return;
        }
        let mass = rod_mass(ends, arms, axis);
        let stretch = length - terms.length;
        let (bias, mass_scale, impulse_scale) = if terms.slack {
            (0.0, 0.0, 0.0)
        } else {
            let (rate, mass_scale, impulse_scale) = soft_terms(sweep);
            (rate * stretch, mass_scale, impulse_scale)
        };
        let speed = closing_rate(ends, arms, axis);
        let impulse = -mass_scale * mass * (speed + bias) - impulse_scale * self.impulses.rod;
        self.impulses.rod += impulse;
        push_at(ends, arms, axis * impulse);
        if let Some(limits) = terms.limits {
            // The rate is read again before each limit: the impulses above changed it.
            let speed = closing_rate(ends, arms, axis);
            let state = (
                mass,
                (length - limits.min).max(-MAX_LIMIT_CORRECTION),
                speed,
            );
            let applied = one_sided(&mut self.impulses.lower, state, sweep);
            push_at(ends, arms, axis * applied);
            let speed = closing_rate(ends, arms, axis);
            let state = (
                mass,
                (limits.max - length).max(-MAX_LIMIT_CORRECTION),
                -speed,
            );
            let applied = one_sided(&mut self.impulses.upper, state, sweep);
            push_at(ends, arms, axis * -applied);
        }
    }
}

/// The bias rate and the two scales of a rigid joint row: the soft values in the solve pass, none in
/// the relax pass.
fn soft_terms(sweep: &JointSweep) -> (f32, f32, f32) {
    let soft: Softness = sweep.rigid;
    if sweep.use_bias {
        (soft.bias_rate, soft.mass_scale, soft.impulse_scale)
    } else {
        (0.0, 1.0, 0.0)
    }
}

/// The speed at which the second anchor moves away from the first along `axis`.
fn closing_rate(ends: &Ends, arms: (Vec3, Vec3), axis: Vec3) -> f32 {
    axis.dot(ends.b.velocity_at(arms.1) - ends.a.velocity_at(arms.0))
}

/// The sum of the inverse inertias of the two bodies applied to `vector`.
fn inertia_times(ends: &Ends, vector: Vec3) -> Vec3 {
    ends.a.inv_inertia * vector + ends.b.inv_inertia * vector
}

/// How a unit torque about `axis` (on the second body, its opposite on the first) changes the
/// relative velocity of the two anchors.
fn coupling_of(ends: &Ends, arms: (Vec3, Vec3), axis: Vec3) -> Vec3 {
    (ends.b.inv_inertia * axis).cross(arms.1) + (ends.a.inv_inertia * axis).cross(arms.0)
}

/// Turns the second body about the hinge axis by the angular impulse `amount` and applies at the
/// anchors the impulse that keeps them from moving relative to each other, so that the change
/// is exactly the one the reduced mass of [`Held`] describes. Applying the torque alone, as the
/// mass assumes the anchors do not move, overshoots by the ratio of the two masses.
fn turn_held(ends: &mut Ends, arms: (Vec3, Vec3), held: &Held, amount: f32) {
    push_spin(ends, held.axis * amount);
    push_at(ends, arms, held.reaction(amount));
}

/// The hinge angle about `axis` with the anchors held together: the unit torque's effect on the
/// anchors and the inverse of the point mass that answers it.
struct Held {
    axis: Vec3,
    /// How a unit torque about `axis` changes the relative velocity of the anchors.
    coupling: Vec3,
    /// The point mass matrix and its inverse (`None` when it has none).
    matrix: Mat3,
    inverse: Option<Mat3>,
}

/// The reduced mass of a hinge angle is never taken above this many times the inertia of the bodies
/// about the axis: past that the two terms it is the difference of (the inertia about the axis and
/// what the anchors take) cancel to nothing in `f32`, and a noise-sized difference would give a
/// huge mass and a limit impulse that throws the body.
const MOST_REDUCED: f32 = 1.0e-3;

impl Held {
    fn new(ends: &Ends, arms: (Vec3, Vec3), axis: Vec3) -> Self {
        let matrix = point_mass(ends, arms);
        Self {
            axis,
            coupling: coupling_of(ends, arms, axis),
            matrix,
            inverse: matrix.inverse(),
        }
    }

    /// `matrix` solved for `rhs`, with one step of refinement: a small body on a long arm makes the
    /// matrix badly conditioned (the tangential entries are thousands of times the entry along the
    /// arm), and the plain inverse then has an error as large as the effect being computed.
    fn solved(&self, rhs: Vec3) -> Vec3 {
        self.inverse.map_or(Vec3::ZERO, |inverse| {
            let first = inverse * rhs;
            first + inverse * (rhs - self.matrix * first)
        })
    }

    /// The impulse at the anchors that cancels what a torque of `amount` about the axis does to
    /// their relative velocity. It is not stored: the warm start computes it again from the limit
    /// impulses and the arms of the moment (a stored copy would go stale as the body moves).
    fn reaction(&self, amount: f32) -> Vec3 {
        self.solved(self.coupling * -amount)
    }

    /// The effective mass of the angle: the bodies' inertia about the axis less what the anchors
    /// take (the Schur complement of the point rows). The anchors hold the arm from the hinge point
    /// to the centre of a body, so a torque turns the whole body about the hinge, not just its spin.
    fn mass(&self, ends: &Ends) -> f32 {
        let own = self.axis.dot(inertia_times(ends, self.axis));
        let held = self.coupling.dot(self.solved(self.coupling));
        inverse_or_zero((own - held).max(own * MOST_REDUCED))
    }
}

/// Applies `impulse` as a torque to the second body and its opposite to the first.
fn push_spin(ends: &mut Ends, impulse: Vec3) {
    ends.a.spin -= ends.a.inv_inertia * impulse;
    ends.b.spin += ends.b.inv_inertia * impulse;
}

/// Applies `impulse` at the second anchor and its opposite at the first.
fn push_at(ends: &mut Ends, arms: (Vec3, Vec3), impulse: Vec3) {
    ends.a.push(arms.0, -impulse);
    ends.b.push(arms.1, impulse);
}

/// The unit direction of `separation` and its length; no direction for a length that is not above
/// [`NEGLIGIBLE_LENGTH`].
fn axis_of(separation: Vec3) -> (Vec3, f32) {
    let length = separation.length();
    if length > NEGLIGIBLE_LENGTH {
        (separation * (1.0 / length), length)
    } else {
        (Vec3::ZERO, length)
    }
}

/// The effective mass of the two bodies along `axis` at the anchors; zero when nothing can move.
fn rod_mass(ends: &Ends, arms: (Vec3, Vec3), axis: Vec3) -> f32 {
    let (ca, cb) = (arms.0.cross(axis), arms.1.cross(axis));
    let k = ends.a.inv_mass
        + ends.b.inv_mass
        + ca.dot(ends.a.inv_inertia * ca)
        + cb.dot(ends.b.inv_inertia * cb);
    inverse_or_zero(k)
}

/// The matrix that turns an impulse at the two anchors into the change of their relative velocity:
/// `m I - [ra] Ia [ra] - [rb] Ib [rb]`, with `[r]` the cross product with `r`.
fn point_mass(ends: &Ends, arms: (Vec3, Vec3)) -> Mat3 {
    let mass = ends.a.inv_mass + ends.b.inv_mass;
    let column = |unit: Vec3| {
        unit * mass
            - arms.0.cross(ends.a.inv_inertia * arms.0.cross(unit))
            - arms.1.cross(ends.b.inv_inertia * arms.1.cross(unit))
    };
    Mat3::from_cols(column(Vec3::X), column(Vec3::Y), column(Vec3::Z))
}

/// Solves the block system `[A B; Bt D] (p, a) = (rp, ra)` with `A` 3 by 3 (given as its inverse,
/// `None` when it has none), `B` 3 by 2 (as its two columns) and `D` 2 by 2, by eliminating `p`
/// (the Schur complement of `A`). Zero when `A` or the complement is singular.
fn block_solve(
    a_inverse: Option<Mat3>,
    b: [Vec3; 2],
    d: [[f32; 2]; 2],
    (rp, ra): (Vec3, [f32; 2]),
) -> (Vec3, [f32; 2]) {
    let Some(inverse) = a_inverse else {
        return (Vec3::ZERO, [0.0; 2]);
    };
    let [b0, b1] = b;
    let (c0, c1) = (inverse * b0, inverse * b1);
    let schur = [
        [d[0][0] - b0.dot(c0), d[0][1] - b0.dot(c1)],
        [d[1][0] - b1.dot(c0), d[1][1] - b1.dot(c1)],
    ];
    let reduced = inverse * rp;
    let second = solve_two(schur, [ra[0] - b0.dot(reduced), ra[1] - b1.dot(reduced)]);
    (reduced - c0 * second[0] - c1 * second[1], second)
}

/// Solves the symmetric 2 by 2 system `k x = rhs`; zero when it is singular.
fn solve_two(k: [[f32; 2]; 2], rhs: [f32; 2]) -> [f32; 2] {
    let det = k[0][0] * k[1][1] - k[0][1] * k[1][0];
    let inverse = inverse_or_zero(det);
    [
        inverse * (k[1][1] * rhs[0] - k[0][1] * rhs[1]),
        inverse * (k[0][0] * rhs[1] - k[1][0] * rhs[0]),
    ]
}

/// The impulses of one joint, with the handle that tells it from a later joint in the same slot.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CachedJoint {
    handle: JointHandle<D3>,
    impulses: JointImpulses,
}

/// The impulses of the last step's joints, in joint-slot order, found with a cursor. Hashes nothing
/// while empty, so a world without joints hashes as before.
struct JointWarm {
    entries: Vec<CachedJoint>,
    capacity: usize,
}

impl JointWarm {
    fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            capacity,
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
    }

    /// Stores the impulses of `handle` unless the buffer is full (it cannot be: one entry per joint,
    /// and the capacity is the joint capacity).
    fn push(&mut self, handle: JointHandle<D3>, impulses: JointImpulses) {
        if self.entries.len() < self.capacity {
            self.entries.push(CachedJoint { handle, impulses });
        }
    }

    /// The impulses kept for `handle` at or after `*cursor`, moving the cursor past the entries of
    /// lower slots. Asked for handles in increasing slot order, as the joint list is.
    fn find(&self, cursor: &mut usize, handle: JointHandle<D3>) -> Option<JointImpulses> {
        let lower = self
            .entries
            .get(*cursor..)?
            .iter()
            .take_while(|entry| entry.handle.index() < handle.index())
            .count();
        *cursor += lower;
        let entry = self.entries.get(*cursor)?;
        (entry.handle == handle).then_some(entry.impulses)
    }

    fn is_finite(&self) -> bool {
        self.entries.iter().all(|entry| entry.impulses.is_finite())
    }

    fn write_bits(&self, hasher: &mut StateHasher) {
        if self.entries.is_empty() {
            return;
        }
        hasher.write_u64(self.entries.len() as u64);
        for entry in &self.entries {
            hasher.write_u32(entry.handle.index());
            hasher.write_u32(entry.handle.generation());
            entry.impulses.write_bits(hasher);
        }
    }
}

/// The joints of a step, with the warm start they carry from step to step.
pub(super) struct JointSet {
    constraints: Vec<JointConstraint>,
    capacity: usize,
    warm: JointWarm,
    next: JointWarm,
    /// Where the warm cache has been read up to (joints come in slot order).
    cursor: usize,
}

impl JointSet {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            constraints: Vec::with_capacity(capacity),
            capacity,
            warm: JointWarm::new(capacity),
            next: JointWarm::new(capacity),
            cursor: 0,
        }
    }

    pub(super) fn clear(&mut self) {
        self.constraints.clear();
        self.cursor = 0;
    }

    /// Adds `constraint` with the impulses of the same joint from the last step; a joint past the
    /// capacity (cannot happen: the world's capacity is the same) is left out.
    pub(super) fn add(&mut self, mut constraint: JointConstraint) {
        if self.constraints.len() >= self.capacity {
            return;
        }
        if let Some(cached) = self.warm.find(&mut self.cursor, constraint.handle) {
            constraint.impulses = cached;
        }
        self.constraints.push(constraint);
    }

    /// Runs `pass` over the joints that are solved, in slot order.
    pub(super) fn sweep(&mut self, bodies: &mut [SolverBody], pass: JointPass) {
        for joint in self.constraints.iter_mut().filter(|joint| joint.solved) {
            visit(bodies, joint, pass);
        }
    }

    /// How many joints are solved and how many are kept as they are.
    pub(super) fn counts(&self) -> (usize, usize) {
        let solved = self.constraints.iter().filter(|joint| joint.solved).count();
        (solved, self.constraints.len() - solved)
    }

    /// Keeps the impulses for the next step.
    pub(super) fn finish(&mut self) {
        self.next.clear();
        for joint in &self.constraints {
            self.next.push(joint.handle, joint.impulses);
        }
    }

    pub(super) fn is_finite(&self) -> bool {
        self.next.is_finite()
    }

    pub(super) fn commit(&mut self) {
        std::mem::swap(&mut self.warm, &mut self.next);
    }

    pub(super) fn write_bits(&self, hasher: &mut StateHasher) {
        self.warm.write_bits(hasher);
    }
}

/// Runs `pass` on `joint` with copies of its two bodies, which are stored back afterwards; a static
/// body is not written back (it did not change).
fn visit(bodies: &mut [SolverBody], joint: &mut JointConstraint, pass: JointPass) {
    let (Some(&a), Some(&b)) = (bodies.get(joint.a), bodies.get(joint.b)) else {
        return;
    };
    let (mut a, mut b) = (a, b);
    let mut ends = Ends {
        a: &mut a,
        b: &mut b,
    };
    match pass {
        JointPass::WarmStart => joint.warm_start(&mut ends),
        JointPass::Solve(sweep) => joint.solve(&mut ends, sweep),
    }
    for (index, body) in [(joint.a, a), (joint.b, b)] {
        if let Some(target) = bodies.get_mut(index).filter(|_| !body.is_static()) {
            *target = body;
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::float_cmp,
    reason = "the values compared are exact by construction: sums of small integers, or zero"
)]
mod tests {
    use super::*;
    use crate::core::PhysicsError;
    use crate::d3::{BodyDesc, BodyKind, Shape3, World, WorldConfig};
    use base::math::Quat;
    use base::pool::Handle;
    use std::f32::consts::FRAC_PI_2;

    const H: f32 = 1.0 / 240.0;

    fn near(a: Vec3, b: Vec3) -> bool {
        (a - b).length() <= 1.0e-5 * b.length().max(1.0)
    }

    fn sweep(use_bias: bool) -> JointSweep {
        JointSweep {
            h: H,
            inv_h: 1.0 / H,
            use_bias,
            rigid: Softness {
                bias_rate: 10.0,
                mass_scale: 0.5,
                impulse_scale: 0.25,
            },
        }
    }

    fn body(inv_mass: f32, inv_inertia: f32) -> SolverBody {
        let mut body = SolverBody::at_rest();
        body.inv_mass = inv_mass;
        body.inv_inertia = Mat3::from_scale(Vec3::new(inv_inertia, inv_inertia, inv_inertia));
        body
    }

    fn handle(slot: u32) -> JointHandle<D3> {
        Handle::from_raw(slot, 0)
    }

    /// A ball joint (anchors on the centres) between bodies `distance` apart on the x axis.
    fn joint_of(kind: Kind, distance: f32, anchors: (Vec3, Vec3)) -> JointConstraint {
        JointConstraint {
            a: 0,
            b: 1,
            handle: handle(0),
            solved: true,
            centres: Vec3::new(distance, 0.0, 0.0),
            anchor_a: anchors.0,
            anchor_b: anchors.1,
            kind,
            impulses: JointImpulses::NONE,
        }
    }

    fn solved_pair(
        joint: &mut JointConstraint,
        mut a: SolverBody,
        mut b: SolverBody,
        use_bias: bool,
    ) -> [SolverBody; 2] {
        joint.solve(
            &mut Ends {
                a: &mut a,
                b: &mut b,
            },
            &sweep(use_bias),
        );
        [a, b]
    }

    fn hinge_about_z() -> Kind {
        Kind::Hinge(HingeTerms {
            axis_a: Vec3::Z,
            axis_b: Vec3::Z,
            reference_a: Vec3::X,
            reference_b: Vec3::X,
            limits: None,
        })
    }

    #[test]
    fn the_hinge_angle_is_the_turn_from_the_first_direction_to_the_second_about_the_axis() {
        let turned = |angle: f32| Vec3::new(dmath::sin_cos(angle).1, dmath::sin_cos(angle).0, 0.0);
        assert!(hinge_angle(Vec3::Z, Vec3::X, Vec3::X).abs() < 1e-6);
        assert!((hinge_angle(Vec3::Z, Vec3::X, turned(0.5)) - 0.5).abs() < 1e-5);
        assert!((hinge_angle(Vec3::Z, turned(0.5), Vec3::X) + 0.5).abs() < 1e-5);
        // Seen from the other side of the axis the same turn has the other sign.
        assert!((hinge_angle(-Vec3::Z, Vec3::X, turned(0.5)) + 0.5).abs() < 1e-5);
    }

    #[test]
    fn a_direction_is_carried_by_the_smallest_rotation_between_two_axes() {
        // Equal axes leave it alone; z to x is a quarter turn about y, which takes x to minus z.
        assert!(near(carried(Vec3::X, Vec3::Z, Vec3::Z), Vec3::X));
        assert!(near(carried(Vec3::X, Vec3::Z, Vec3::X), -Vec3::Z));
        assert!(near(carried(Vec3::Y, Vec3::Z, Vec3::X), Vec3::Y));
        // Opposite axes: a half turn about the direction itself, which leaves it alone.
        assert!(near(carried(Vec3::X, Vec3::Z, -Vec3::Z), Vec3::X));
        // Just short of opposite is still a finite, unit result.
        let nearly = Vec3::new(0.0, 0.05, -1.0).normalized().unwrap_or(-Vec3::Z);
        let result = carried(Vec3::X, Vec3::Z, nearly);
        assert!((result.length() - 1.0).abs() < 1e-4, "{result:?}");
    }

    #[test]
    fn a_direction_is_turned_counter_clockwise_about_the_axis() {
        assert!(near(turned_about(Vec3::X, Vec3::Z, FRAC_PI_2), Vec3::Y));
        assert!(near(turned_about(Vec3::X, Vec3::Z, -FRAC_PI_2), -Vec3::Y));
        assert!(near(turned_about(Vec3::Y, Vec3::X, FRAC_PI_2), Vec3::Z));
        assert!(near(turned_about(Vec3::X, Vec3::Z, 0.0), Vec3::X));
    }

    #[test]
    fn a_rigid_row_has_the_soft_terms_in_the_solve_pass_and_none_in_the_relax_pass() {
        assert_eq!(soft_terms(&sweep(true)), (10.0, 0.5, 0.25));
        assert_eq!(soft_terms(&sweep(false)), (0.0, 1.0, 0.0));
    }

    #[test]
    fn a_separation_without_a_direction_has_no_axis() {
        assert_eq!(axis_of(Vec3::new(0.0, 0.0, 3.0)), (Vec3::Z, 3.0));
        let (axis, length) = axis_of(Vec3::new(0.0, 5.0e-7, 0.0));
        assert_eq!(axis, Vec3::ZERO);
        assert_eq!(length, 5.0e-7);
        let (axis, _) = axis_of(Vec3::new(2.0e-6, 0.0, 0.0));
        assert_eq!(axis, Vec3::X);
    }

    #[test]
    fn the_point_mass_of_a_body_with_an_arm_has_the_inertia_in_the_other_directions() {
        // A static first body, a second of mass 1 and unit inertia whose anchor is 1 m from its
        // centre along y: a push along x or z also spins it, a push along y does not.
        let (mut a, mut b) = (body(0.0, 0.0), body(1.0, 1.0));
        let ends = Ends {
            a: &mut a,
            b: &mut b,
        };
        let matrix = point_mass(&ends, (Vec3::ZERO, Vec3::Y));
        assert_eq!(
            matrix.cols,
            [Vec3::new(2.0, 0.0, 0.0), Vec3::Y, Vec3::new(0.0, 0.0, 2.0)]
        );
        // The first body's arm counts the same way.
        let (mut a, mut b) = (body(1.0, 1.0), body(0.0, 0.0));
        let ends = Ends {
            a: &mut a,
            b: &mut b,
        };
        let matrix = point_mass(&ends, (Vec3::Y, Vec3::ZERO));
        assert_eq!(
            matrix.cols,
            [Vec3::new(2.0, 0.0, 0.0), Vec3::Y, Vec3::new(0.0, 0.0, 2.0)]
        );
    }

    #[test]
    fn a_torque_moves_the_anchors_by_the_arm_of_each_body() {
        let (mut a, mut b) = (body(0.0, 0.0), body(1.0, 2.0));
        let ends = Ends {
            a: &mut a,
            b: &mut b,
        };
        // inv inertia 2 turns the torque about z into a spin of 2 about z; at 1 m along y the
        // anchor of the second body moves by spin x arm = (0, 0, 2) x (0, 1, 0) = (-2, 0, 0).
        let coupling = coupling_of(&ends, (Vec3::ZERO, Vec3::Y), Vec3::Z);
        assert!(near(coupling, Vec3::new(-2.0, 0.0, 0.0)));
        let (mut a, mut b) = (body(1.0, 2.0), body(0.0, 0.0));
        let ends = Ends {
            a: &mut a,
            b: &mut b,
        };
        let coupling = coupling_of(&ends, (Vec3::Y, Vec3::ZERO), Vec3::Z);
        assert!(near(coupling, Vec3::new(-2.0, 0.0, 0.0)));
    }

    #[test]
    fn the_mass_of_a_hinge_angle_counts_the_arm_of_the_body_that_turns() {
        // A static pivot and a body of mass 1 and unit inertia 1 m from it: turned about the hinge
        // it has the inertia of a body of that mass at that radius, 1 + 1 x 1 x 1 = 2.
        let (mut a, mut b) = (body(0.0, 0.0), body(1.0, 1.0));
        let ends = Ends {
            a: &mut a,
            b: &mut b,
        };
        let mass = Held::new(&ends, (Vec3::ZERO, -Vec3::X), Vec3::Z).mass(&ends);
        assert!((mass - 2.0).abs() < 1e-5, "{mass}");
        // Without an arm it is the inertia alone.
        let mass = Held::new(&ends, (Vec3::ZERO, Vec3::ZERO), Vec3::Z).mass(&ends);
        assert!((mass - 1.0).abs() < 1e-5, "{mass}");
        // Two bodies that cannot turn have no mass to give.
        let (mut a, mut b) = (body(1.0, 0.0), body(1.0, 0.0));
        let ends = Ends {
            a: &mut a,
            b: &mut b,
        };
        assert_eq!(
            Held::new(&ends, (Vec3::ZERO, Vec3::ZERO), Vec3::Z).mass(&ends),
            0.0
        );
    }

    #[test]
    fn turning_a_held_hinge_changes_the_angle_rate_by_the_reduced_mass_and_leaves_the_anchors_alone()
     {
        // A static pivot and a body of mass 1, inertia 1/0.004 (a small sphere) whose anchor is 1 m
        // from its centre: the anchor stays where it is and the body swings about it.
        let (mut a, mut b) = (body(0.0, 0.0), body(1.0, 250.0));
        let arms = (Vec3::ZERO, -Vec3::X);
        let held = Held::new(
            &Ends {
                a: &mut a,
                b: &mut b,
            },
            arms,
            Vec3::Z,
        );
        let mut ends = Ends {
            a: &mut a,
            b: &mut b,
        };
        turn_held(&mut ends, arms, &held, 1.0);
        // Reduced inertia about the pivot is 1 / 250 + 1 x 1 = 1.004: the rate changes by 1/1.004.
        let rate = ends.b.spin.z;
        assert!((rate - 1.0 / 1.004).abs() < 1e-3, "{rate}");
        let anchor = ends.b.velocity_at(arms.1) - ends.a.velocity_at(arms.0);
        assert!(anchor.length() < 1e-4, "{anchor:?}");
    }

    #[test]
    fn a_rod_has_the_effective_mass_of_the_two_bodies_along_its_axis() {
        let (mut a, mut b) = (body(1.0, 0.0), body(1.0, 0.0));
        let ends = Ends {
            a: &mut a,
            b: &mut b,
        };
        assert_eq!(rod_mass(&ends, (Vec3::ZERO, Vec3::ZERO), Vec3::X), 0.5);
        // An arm across the axis adds the inertia: 0 + 1 + 1 x (1 x 1).
        let (mut a, mut b) = (body(0.0, 0.0), body(1.0, 1.0));
        let ends = Ends {
            a: &mut a,
            b: &mut b,
        };
        assert_eq!(rod_mass(&ends, (Vec3::ZERO, Vec3::Y), Vec3::X), 0.5);
        let (mut a, mut b) = (body(0.0, 1.0), body(0.0, 1.0));
        let ends = Ends {
            a: &mut a,
            b: &mut b,
        };
        assert_eq!(rod_mass(&ends, (Vec3::ZERO, Vec3::ZERO), Vec3::X), 0.0);
    }

    #[test]
    fn a_two_by_two_system_is_solved_with_its_off_diagonal() {
        assert_eq!(solve_two([[2.0, 0.0], [0.0, 4.0]], [2.0, 4.0]), [1.0, 1.0]);
        assert_eq!(solve_two([[2.0, 1.0], [1.0, 2.0]], [3.0, 3.0]), [1.0, 1.0]);
        let [x, y] = solve_two([[2.0, 1.0], [1.0, 2.0]], [1.0, 0.0]);
        assert!((x - 2.0 / 3.0).abs() < 1e-6 && (y + 1.0 / 3.0).abs() < 1e-6);
        assert_eq!(solve_two([[1.0, 1.0], [1.0, 1.0]], [1.0, 1.0]), [0.0, 0.0]);
    }

    #[test]
    fn a_block_system_is_solved_with_its_coupling_and_is_zero_when_singular() {
        // p + a0 X = (3, 0, 0), p.x + 2 a0 = 4, a1 = 5: a0 = 1, p = (2, 0, 0).
        let solved = block_solve(
            Some(Mat3::IDENTITY),
            [Vec3::X, Vec3::ZERO],
            [[2.0, 0.0], [0.0, 1.0]],
            (Vec3::new(3.0, 0.0, 0.0), [4.0, 5.0]),
        );
        assert_eq!(solved, (Vec3::new(2.0, 0.0, 0.0), [1.0, 5.0]));
        // The second column couples the second row to the y component.
        let solved = block_solve(
            Some(Mat3::IDENTITY),
            [Vec3::ZERO, Vec3::Y],
            [[1.0, 0.0], [0.0, 2.0]],
            (Vec3::new(0.0, 3.0, 0.0), [7.0, 4.0]),
        );
        assert_eq!(solved, (Vec3::new(0.0, 2.0, 0.0), [7.0, 1.0]));
        let none = block_solve(
            None,
            [Vec3::X, Vec3::Y],
            [[1.0, 0.0], [0.0, 1.0]],
            (Vec3::X, [1.0, 1.0]),
        );
        assert_eq!(none, (Vec3::ZERO, [0.0, 0.0]));
    }

    #[test]
    fn a_ball_joint_stops_the_relative_velocity_of_its_anchors() {
        let mut joint = joint_of(Kind::Ball, 0.0, (Vec3::ZERO, Vec3::ZERO));
        let mut moving = body(1.0, 0.0);
        moving.velocity = Vec3::new(1.0, 0.0, 0.0);
        let [first, second] = solved_pair(&mut joint, body(0.0, 0.0), moving, false);
        assert_eq!(first.velocity, Vec3::ZERO);
        assert!(near(second.velocity, Vec3::ZERO));
        assert!(near(joint.impulses.point, Vec3::new(-1.0, 0.0, 0.0)));
    }

    #[test]
    fn a_ball_joint_pulls_the_anchors_together_with_the_bias_of_the_solve_pass() {
        // The anchors are 2 m apart along x and at rest: the bias speed is 10 x 2 = 20, the mass
        // scale 0.5, so the second body (mass 1) gets the impulse -0.5 x 20.
        let mut joint = joint_of(Kind::Ball, 2.0, (Vec3::ZERO, Vec3::ZERO));
        let [_, second] = solved_pair(&mut joint, body(0.0, 0.0), body(1.0, 0.0), true);
        assert!(
            near(second.velocity, Vec3::new(-10.0, 0.0, 0.0)),
            "{:?}",
            second.velocity
        );
        // The relax pass has no bias.
        let mut joint = joint_of(Kind::Ball, 2.0, (Vec3::ZERO, Vec3::ZERO));
        let [_, second] = solved_pair(&mut joint, body(0.0, 0.0), body(1.0, 0.0), false);
        assert_eq!(second.velocity, Vec3::ZERO);
    }

    #[test]
    fn a_ball_joint_remembers_part_of_its_impulse_in_the_soft_pass() {
        let mut joint = joint_of(Kind::Ball, 0.0, (Vec3::ZERO, Vec3::ZERO));
        joint.impulses.point = Vec3::new(4.0, 0.0, 0.0);
        let mut moving = body(1.0, 0.0);
        moving.velocity = Vec3::new(2.0, 0.0, 0.0);
        let [_, second] = solved_pair(&mut joint, body(0.0, 0.0), moving, true);
        // impulse = -(m 2 x 0.5) - 4 x 0.25 = -2; the total is 4 - 2.
        assert!(near(joint.impulses.point, Vec3::new(2.0, 0.0, 0.0)));
        assert!(near(second.velocity, Vec3::new(0.0, 0.0, 0.0)));
    }

    #[test]
    fn a_hinge_removes_the_spin_across_its_axis_and_keeps_the_spin_about_it() {
        let mut joint = joint_of(hinge_about_z(), 0.0, (Vec3::ZERO, Vec3::ZERO));
        let mut turning = body(1.0, 1.0);
        turning.spin = Vec3::new(1.0, 2.0, 3.0);
        let [_, second] = solved_pair(&mut joint, body(0.0, 0.0), turning, false);
        assert!(
            near(second.spin, Vec3::new(0.0, 0.0, 3.0)),
            "{:?}",
            second.spin
        );
        assert!(joint.impulses.align.dot(Vec3::Z).abs() < 1e-6);
        assert!(near(joint.impulses.align, Vec3::new(-1.0, -2.0, 0.0)));
    }

    #[test]
    fn a_tilted_axis_gets_a_correcting_spin_in_the_solve_pass() {
        // The second axis is tilted towards +x seen from the first: the cross product of the axes
        // is (0, 1, 0) x ... the error is axis_a x axis_b.
        let tilted = Vec3::new(0.0, 0.1, 1.0);
        let kind = Kind::Hinge(HingeTerms {
            axis_b: tilted,
            ..match hinge_about_z() {
                Kind::Hinge(terms) => terms,
                other => panic!("not a hinge: {other:?}"),
            }
        });
        let mut joint = joint_of(kind, 0.0, (Vec3::ZERO, Vec3::ZERO));
        let [_, second] = solved_pair(&mut joint, body(0.0, 0.0), body(1.0, 1.0), true);
        // error = Z x (0, 0.1, 1) = (-0.1, 0, 0); bias 10, mass scale 0.5, unit inertia.
        assert!(
            near(second.spin, Vec3::new(0.5, 0.0, 0.0)),
            "{:?}",
            second.spin
        );
    }

    #[test]
    fn a_hinge_with_an_arm_turns_the_spin_error_into_a_point_error_and_removes_both() {
        // The second body's anchor is 1 m from its centre along -x, the pivot is fixed: the centre
        // may only move across the arm. A velocity along z and a spin about y are one motion.
        let mut joint = joint_of(hinge_about_z(), 0.0, (Vec3::ZERO, -Vec3::X));
        let mut swinging = body(1.0, 1.0);
        swinging.velocity = Vec3::new(0.0, 0.0, 2.0);
        let [_, second] = solved_pair(&mut joint, body(0.0, 0.0), swinging, false);
        let anchor = second.velocity_at(-Vec3::X);
        assert!(near(anchor, Vec3::ZERO), "anchor {anchor:?}");
        assert!(
            second.spin.x.abs() < 1e-5 && second.spin.y.abs() < 1e-5,
            "{:?}",
            second.spin
        );
    }

    #[test]
    fn a_hinge_limit_pushes_back_a_body_that_is_past_it() {
        let limit = |angle_error_spin: f32| {
            let mut terms = match hinge_about_z() {
                Kind::Hinge(terms) => terms,
                other => panic!("not a hinge: {other:?}"),
            };
            terms.limits = Some(AngleLimits {
                lower: -0.5,
                upper: 0.5,
            });
            // The second reference is turned to 0.7 rad: past the upper limit by 0.2.
            terms.reference_b = Vec3::new(dmath::sin_cos(0.7).1, dmath::sin_cos(0.7).0, 0.0);
            let mut joint = joint_of(Kind::Hinge(terms), 0.0, (Vec3::ZERO, Vec3::ZERO));
            let mut turning = body(1.0, 1.0);
            turning.spin = Vec3::new(0.0, 0.0, angle_error_spin);
            let [_, second] = solved_pair(&mut joint, body(0.0, 0.0), turning, false);
            (second.spin.z, joint.impulses)
        };
        // Turning further past the upper limit at 2 rad/s: the spin is stopped, the upper impulse
        // accumulates, the lower stays zero.
        let (spin, impulses) = limit(2.0);
        assert!(spin.abs() < 1e-5, "{spin}");
        assert!(
            (impulses.upper - 2.0).abs() < 1e-5 && impulses.lower == 0.0,
            "{impulses:?}"
        );
        // Turning back needs no push.
        let (spin, impulses) = limit(-2.0);
        assert_eq!(spin, -2.0);
        assert_eq!(impulses.upper, 0.0);
    }

    #[test]
    fn a_hinge_limit_below_the_lower_stop_pushes_the_other_way() {
        let mut terms = match hinge_about_z() {
            Kind::Hinge(terms) => terms,
            other => panic!("not a hinge: {other:?}"),
        };
        terms.limits = Some(AngleLimits {
            lower: -0.5,
            upper: 0.5,
        });
        terms.reference_b = Vec3::new(dmath::sin_cos(-0.7).1, dmath::sin_cos(-0.7).0, 0.0);
        let mut joint = joint_of(Kind::Hinge(terms), 0.0, (Vec3::ZERO, Vec3::ZERO));
        let mut turning = body(1.0, 1.0);
        turning.spin = Vec3::new(0.0, 0.0, -2.0);
        let [_, second] = solved_pair(&mut joint, body(0.0, 0.0), turning, false);
        assert!(second.spin.z.abs() < 1e-5, "{}", second.spin.z);
        assert!((joint.impulses.lower - 2.0).abs() < 1e-5 && joint.impulses.upper == 0.0);
    }

    #[test]
    fn a_limit_violated_far_is_corrected_as_if_it_were_violated_by_the_most_it_counts() {
        let hinge_push = |past: f32| {
            let mut terms = match hinge_about_z() {
                Kind::Hinge(terms) => terms,
                other => panic!("not a hinge: {other:?}"),
            };
            terms.limits = Some(AngleLimits {
                lower: -0.5,
                upper: 0.5,
            });
            let angle = 0.5 + past;
            terms.reference_b = Vec3::new(dmath::sin_cos(angle).1, dmath::sin_cos(angle).0, 0.0);
            let mut joint = joint_of(Kind::Hinge(terms), 0.0, (Vec3::ZERO, Vec3::ZERO));
            let [_, second] = solved_pair(&mut joint, body(0.0, 0.0), body(1.0, 1.0), true);
            (joint.impulses.upper, second.spin.z)
        };
        let (near_push, near_spin) = hinge_push(MAX_LIMIT_CORRECTION);
        let (far_push, far_spin) = hinge_push(1.0);
        assert!(near_push > 0.05, "{near_push}");
        assert!((near_push - far_push).abs() < 1e-4 && (near_spin - far_spin).abs() < 1e-4);
        // Below the most it counts the push grows with the error.
        assert!(hinge_push(0.01).0 < near_push);

        let rope_push = |length: f32| {
            let terms = RodTerms {
                length: 2.0,
                slack: true,
                limits: Some(LengthLimits { min: 1.0, max: 3.0 }),
            };
            let mut joint = joint_of(Kind::Rod(terms), length, (Vec3::ZERO, Vec3::ZERO));
            solved_pair(&mut joint, body(0.0, 0.0), body(1.0, 0.0), true);
            joint.impulses.upper
        };
        assert!(rope_push(3.0 + MAX_LIMIT_CORRECTION) > 0.02);
        assert!((rope_push(3.0 + MAX_LIMIT_CORRECTION) - rope_push(5.0)).abs() < 1e-4);
        assert!(rope_push(3.01) < rope_push(3.0 + MAX_LIMIT_CORRECTION));
        let short_push = |length: f32| {
            let terms = RodTerms {
                length: 2.0,
                slack: true,
                limits: Some(LengthLimits { min: 1.0, max: 3.0 }),
            };
            let mut joint = joint_of(Kind::Rod(terms), length, (Vec3::ZERO, Vec3::ZERO));
            solved_pair(&mut joint, body(0.0, 0.0), body(1.0, 0.0), true);
            joint.impulses.lower
        };
        assert!(short_push(1.0 - MAX_LIMIT_CORRECTION) > 0.02);
        assert!((short_push(1.0 - MAX_LIMIT_CORRECTION) - short_push(0.2)).abs() < 1e-4);
    }

    #[test]
    fn a_direction_is_carried_exactly_between_axes_at_sixty_degrees_and_near_opposite() {
        let sixty = Vec3::new(0.5, 0.866_025_4, 0.0);
        assert!(near(
            carried(Vec3::Y, Vec3::X, sixty),
            Vec3::new(-0.866_025_4, 0.5, 0.0)
        ));
        // 172 degrees: inside the range that is carried (the limit is a cosine of -0.999).
        let wide = Vec3::new(-0.99, 0.141_067_36, 0.0);
        let result = carried(Vec3::Y, Vec3::X, wide);
        assert!(
            (result - Vec3::new(-0.141_067_36, -0.99, 0.0)).length() < 2e-3,
            "{result:?}"
        );
    }

    #[test]
    fn bodies_that_move_together_get_no_impulse_from_a_hinge_or_its_limit() {
        let mut terms = match hinge_about_z() {
            Kind::Hinge(terms) => terms,
            other => panic!("not a hinge: {other:?}"),
        };
        terms.limits = Some(AngleLimits {
            lower: -0.5,
            upper: 0.5,
        });
        terms.reference_b = Vec3::new(dmath::sin_cos(-0.7).1, dmath::sin_cos(-0.7).0, 0.0);
        let mut joint = joint_of(Kind::Hinge(terms), 0.0, (Vec3::ZERO, Vec3::ZERO));
        let mut together = body(1.0, 1.0);
        together.velocity = Vec3::new(1.0, 2.0, 3.0);
        together.spin = Vec3::new(1.0, 2.0, -2.0);
        let [first, second] = solved_pair(&mut joint, together, together, false);
        assert!(
            near(first.velocity, together.velocity) && near(second.velocity, together.velocity)
        );
        assert!(near(first.spin, together.spin) && near(second.spin, together.spin));
        assert!(joint.impulses.lower.abs() < 1e-6 && joint.impulses.upper.abs() < 1e-6);
    }

    #[test]
    fn a_rod_takes_back_a_part_of_the_impulse_it_carries() {
        let terms = RodTerms {
            length: 2.0,
            slack: false,
            limits: None,
        };
        let mut joint = joint_of(Kind::Rod(terms), 2.0, (Vec3::ZERO, Vec3::ZERO));
        joint.impulses.rod = 1.0;
        solved_pair(&mut joint, body(1.0, 0.0), body(1.0, 0.0), true);
        // impulse_scale 0.25 of the carried 1.0 is given back.
        assert!(
            (joint.impulses.rod - 0.75).abs() < 1e-6,
            "{}",
            joint.impulses.rod
        );
    }

    #[test]
    fn the_inertia_of_the_first_body_counts_in_the_mass_of_a_rod() {
        let (mut a, mut b) = (body(0.0, 1.0), body(0.0, 0.0));
        let ends = Ends {
            a: &mut a,
            b: &mut b,
        };
        assert_eq!(rod_mass(&ends, (Vec3::Y, Vec3::ZERO), Vec3::X), 1.0);
    }

    #[test]
    fn a_separation_exactly_as_long_as_the_smallest_has_no_direction() {
        assert_eq!(
            axis_of(Vec3::new(NEGLIGIBLE_LENGTH, 0.0, 0.0)).0,
            Vec3::ZERO
        );
        assert_eq!(
            axis_of(Vec3::new(2.0 * NEGLIGIBLE_LENGTH, 0.0, 0.0)).0,
            Vec3::X
        );
    }

    #[test]
    fn a_joint_set_reports_non_finite_impulses_and_hashes_the_impulses_it_keeps() {
        let hash = |set: &JointSet| {
            let mut hasher = StateHasher::new();
            set.write_bits(&mut hasher);
            hasher.finish()
        };
        let mut set = JointSet::new(1);
        let mut joint = joint_of(Kind::Ball, 0.0, (Vec3::ZERO, Vec3::ZERO));
        joint.impulses.point = Vec3::new(1.0, 0.0, 0.0);
        set.add(joint);
        set.finish();
        assert!(set.is_finite());
        set.commit();
        let kept = hash(&set);
        assert_ne!(kept, hash(&JointSet::new(1)));
        let mut broken = JointSet::new(1);
        let mut joint = joint_of(Kind::Ball, 0.0, (Vec3::ZERO, Vec3::ZERO));
        joint.impulses.point = Vec3::new(f32::NAN, 0.0, 0.0);
        broken.add(joint);
        broken.finish();
        assert!(!broken.is_finite());
    }

    #[test]
    fn a_block_system_with_coupled_columns_is_solved_with_its_off_diagonal_terms() {
        // The identity as the 3 by 3 block, columns (1, 1, 0) and (0, 1, 0), D = [4 1; 1 3]; built
        // so that p = (1, 2, 3) and a = (1, -1) solve it.
        let one = Mat3::from_scale(Vec3::new(1.0, 1.0, 1.0));
        let (p, a) = block_solve(
            Some(one),
            [Vec3::new(1.0, 1.0, 0.0), Vec3::Y],
            [[4.0, 1.0], [1.0, 3.0]],
            (Vec3::new(2.0, 2.0, 3.0), [6.0, 0.0]),
        );
        assert!(near(p, Vec3::new(1.0, 2.0, 3.0)), "{p:?}");
        assert!(
            (a[0] - 1.0).abs() < 1e-5 && (a[1] + 1.0).abs() < 1e-5,
            "{a:?}"
        );
    }

    #[test]
    fn a_refined_solve_adds_the_correction_of_the_residual() {
        // The "inverse" 0.4 of the matrix 2 is off by a fifth: 0.4, then 0.4 + 0.4 x (1 - 2 x 0.4).
        let held = Held {
            axis: Vec3::Z,
            coupling: Vec3::ZERO,
            matrix: Mat3::from_scale(Vec3::new(2.0, 2.0, 2.0)),
            inverse: Some(Mat3::from_scale(Vec3::new(0.4, 0.4, 0.4))),
        };
        assert!(near(held.solved(Vec3::X), Vec3::new(0.48, 0.0, 0.0)));
    }

    /// The terms of the hinge the joint makes between two spheres turned by `rotation`.
    fn hinge_terms_between_turned_bodies(
        hinge: &HingeJoint3,
        rotation: Quat,
    ) -> Result<HingeTerms, PhysicsError> {
        let mut world = World::new(WorldConfig {
            max_bodies: 2,
            gravity: Vec3::ZERO,
            max_pairs: 4,
            broadphase_margin: crate::d3::DEFAULT_BROADPHASE_MARGIN,
            max_manifolds: 4,
            max_joints: 0,
            speculative_margin: crate::d3::DEFAULT_SPECULATIVE_MARGIN,
            solver: crate::d3::SolverConfig::DEFAULT,
            sleep: crate::d3::SleepConfig::DISABLED,
        })?;
        let mut make = |position| {
            world.create_body(&BodyDesc {
                rotation,
                ..BodyDesc::new(BodyKind::Dynamic, position, Shape3::Sphere { radius: 0.5 })
            })
        };
        let first = make(Vec3::ZERO)?;
        let second = make(Vec3::new(2.0, 0.0, 0.0))?;
        let setup = JointSetup {
            handle: handle(0),
            slots: (0, 1),
            bodies: [world.body(first)?, world.body(second)?],
        };
        match JointConstraint::hinge(&setup, hinge).kind {
            Kind::Hinge(terms) => Ok(terms),
            other => panic!("not a hinge: {other:?}"),
        }
    }

    #[test]
    fn a_hinge_with_opposite_local_axes_aligns_on_the_flipped_second_axis()
    -> Result<(), PhysicsError> {
        let hinge = HingeJoint3::new(Vec3::ZERO, Vec3::ZERO, Vec3::Z, -Vec3::Z);
        // Equal orientations, whatever they are: the aligned axes coincide, so there is no tilt.
        for rotation in [
            Quat::IDENTITY,
            Quat::from_axis_angle(Vec3::new(1.0, 0.3, 0.2), 0.7).unwrap_or(Quat::IDENTITY),
        ] {
            let terms = hinge_terms_between_turned_bodies(&hinge, rotation)?;
            assert!(
                near(terms.axis_a, terms.axis_b),
                "{:?} {:?}",
                terms.axis_a,
                terms.axis_b
            );
        }
        Ok(())
    }

    #[test]
    fn a_rod_holds_its_length_and_ignores_the_speed_across_it() {
        let terms = RodTerms {
            length: 2.0,
            slack: false,
            limits: None,
        };
        let mut joint = joint_of(Kind::Rod(terms), 2.0, (Vec3::ZERO, Vec3::ZERO));
        let mut moving = body(1.0, 0.0);
        moving.velocity = Vec3::new(1.0, 3.0, 0.0);
        let [_, second] = solved_pair(&mut joint, body(0.0, 0.0), moving, false);
        assert!(
            near(second.velocity, Vec3::new(0.0, 3.0, 0.0)),
            "{:?}",
            second.velocity
        );
        assert!((joint.impulses.rod + 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_rod_that_is_too_long_pulls_with_the_bias() {
        let terms = RodTerms {
            length: 2.0,
            slack: false,
            limits: None,
        };
        let mut joint = joint_of(Kind::Rod(terms), 2.5, (Vec3::ZERO, Vec3::ZERO));
        let [_, second] = solved_pair(&mut joint, body(0.0, 0.0), body(1.0, 0.0), true);
        // stretch 0.5 x bias 10 = 5, mass scale 0.5, mass 1: the speed is -2.5 along the axis.
        assert!(
            near(second.velocity, Vec3::new(-2.5, 0.0, 0.0)),
            "{:?}",
            second.velocity
        );
    }

    #[test]
    fn anchors_on_top_of_each_other_give_a_rod_no_direction_and_no_impulse() {
        let terms = RodTerms {
            length: 2.0,
            slack: false,
            limits: None,
        };
        let mut joint = joint_of(Kind::Rod(terms), 0.0, (Vec3::ZERO, Vec3::ZERO));
        let mut moving = body(1.0, 0.0);
        moving.velocity = Vec3::new(1.0, 0.0, 0.0);
        let [_, second] = solved_pair(&mut joint, body(0.0, 0.0), moving, true);
        assert_eq!(second.velocity, Vec3::X);
        assert_eq!(joint.impulses, JointImpulses::NONE);
    }

    #[test]
    fn a_rope_does_nothing_between_its_lengths_and_stops_a_body_at_the_longest() {
        let terms = RodTerms {
            length: 2.0,
            slack: true,
            limits: Some(LengthLimits { min: 1.0, max: 3.0 }),
        };
        let mut moving = body(1.0, 0.0);
        moving.velocity = Vec3::new(1.0, 0.0, 0.0);
        // Slack at 2 m with room of 1 m left and 1 m/s: the speculative limit lets it go on.
        let mut joint = joint_of(Kind::Rod(terms), 2.0, (Vec3::ZERO, Vec3::ZERO));
        let [_, second] = solved_pair(&mut joint, body(0.0, 0.0), moving, false);
        assert_eq!(second.velocity, Vec3::X);
        // At 3 m (the longest) moving away: stopped.
        let mut joint = joint_of(Kind::Rod(terms), 3.0, (Vec3::ZERO, Vec3::ZERO));
        let [_, second] = solved_pair(&mut joint, body(0.0, 0.0), moving, false);
        assert!(near(second.velocity, Vec3::ZERO), "{:?}", second.velocity);
        assert!((joint.impulses.upper - 1.0).abs() < 1e-6 && joint.impulses.lower == 0.0);
        // At 1 m (the shortest) moving closer: stopped on the other side.
        let mut closing = body(1.0, 0.0);
        closing.velocity = Vec3::new(-1.0, 0.0, 0.0);
        let mut joint = joint_of(Kind::Rod(terms), 1.0, (Vec3::ZERO, Vec3::ZERO));
        let [_, second] = solved_pair(&mut joint, body(0.0, 0.0), closing, false);
        assert!(near(second.velocity, Vec3::ZERO), "{:?}", second.velocity);
        assert!((joint.impulses.lower - 1.0).abs() < 1e-6 && joint.impulses.upper == 0.0);
    }

    #[test]
    fn the_warm_start_applies_the_impulses_of_the_last_step_to_both_bodies() {
        let mut ball = joint_of(Kind::Ball, 0.0, (Vec3::ZERO, Vec3::ZERO));
        ball.impulses.point = Vec3::new(2.0, 0.0, 0.0);
        let (mut a, mut b) = (body(0.5, 0.0), body(1.0, 0.0));
        ball.warm_start(&mut Ends {
            a: &mut a,
            b: &mut b,
        });
        assert_eq!(a.velocity, Vec3::new(-1.0, 0.0, 0.0));
        assert_eq!(b.velocity, Vec3::new(2.0, 0.0, 0.0));

        let mut hinge = joint_of(hinge_about_z(), 0.0, (Vec3::ZERO, Vec3::ZERO));
        hinge.impulses.align = Vec3::new(1.0, 0.0, 0.0);
        hinge.impulses.lower = 1.0;
        hinge.impulses.upper = 3.0;
        let (mut a, mut b) = (body(0.0, 1.0), body(0.0, 2.0));
        hinge.warm_start(&mut Ends {
            a: &mut a,
            b: &mut b,
        });
        // Torque (1, 0, 1 - 3): the second body gets it times its inverse inertia, the first the
        // opposite.
        assert!(near(b.spin, Vec3::new(2.0, 0.0, -4.0)), "{:?}", b.spin);
        assert!(near(a.spin, Vec3::new(-1.0, 0.0, 2.0)), "{:?}", a.spin);

        let terms = RodTerms {
            length: 2.0,
            slack: false,
            limits: None,
        };
        let mut rod = joint_of(Kind::Rod(terms), 2.0, (Vec3::ZERO, Vec3::ZERO));
        rod.impulses.rod = 1.0;
        rod.impulses.lower = 0.5;
        rod.impulses.upper = 2.0;
        let (mut a, mut b) = (body(1.0, 0.0), body(1.0, 0.0));
        rod.warm_start(&mut Ends {
            a: &mut a,
            b: &mut b,
        });
        assert_eq!(b.velocity, Vec3::new(-0.5, 0.0, 0.0));
        assert_eq!(a.velocity, Vec3::new(0.5, 0.0, 0.0));
    }

    #[test]
    fn the_arms_and_the_separation_follow_the_turn_and_the_shift_of_the_bodies() {
        let joint = joint_of(Kind::Ball, 2.0, (Vec3::X, Vec3::Y));
        let (mut a, mut b) = (body(1.0, 0.0), body(1.0, 0.0));
        let ends = Ends {
            a: &mut a,
            b: &mut b,
        };
        assert_eq!(joint.arms(&ends), (Vec3::X, Vec3::Y));
        assert_eq!(
            joint.separation(&ends, (Vec3::X, Vec3::Y)),
            Vec3::new(1.0, 1.0, 0.0)
        );
        let mut turned = body(1.0, 0.0);
        turned.turn = Quat::from_axis_angle(Vec3::Z, FRAC_PI_2).unwrap_or(Quat::IDENTITY);
        turned.shift = Vec3::new(0.0, 0.0, 5.0);
        let mut still = body(1.0, 0.0);
        let ends = Ends {
            a: &mut still,
            b: &mut turned,
        };
        let arms = joint.arms(&ends);
        assert!(near(arms.0, Vec3::X) && near(arms.1, -Vec3::X), "{arms:?}");
        let separation = joint.separation(&ends, arms);
        assert!(
            near(separation, Vec3::new(2.0 - 1.0 - 1.0, 0.0, 5.0)),
            "{separation:?}"
        );
    }

    #[test]
    fn a_joint_set_keeps_the_impulses_by_handle_and_only_solves_awake_joints() {
        let mut set = JointSet::new(2);
        let mut first = joint_of(Kind::Ball, 0.0, (Vec3::ZERO, Vec3::ZERO));
        first.handle = handle(0);
        first.impulses.point = Vec3::X;
        let mut second = first;
        second.handle = handle(1);
        second.solved = false;
        set.add(first);
        set.add(second);
        set.add(first);
        assert_eq!(set.counts(), (1, 1), "a third joint is past the capacity");
        set.finish();
        assert!(set.is_finite());
        set.commit();
        // The next step finds the impulses again, by handle, in slot order.
        set.clear();
        let mut fresh = joint_of(Kind::Ball, 0.0, (Vec3::ZERO, Vec3::ZERO));
        fresh.handle = handle(0);
        set.add(fresh);
        let mut other = fresh;
        other.handle = handle(1);
        other.solved = false;
        set.add(other);
        let mut bodies = [body(1.0, 0.0), body(1.0, 0.0)];
        set.sweep(&mut bodies, JointPass::WarmStart);
        // Only the joint of the first handle is solved (the second is frozen) and it carries X.
        assert!(near(bodies[1].velocity, Vec3::X));
        assert_eq!(set.counts(), (1, 1));
    }

    #[test]
    fn a_joint_with_another_generation_does_not_find_the_old_impulses() {
        let mut warm = JointWarm::new(2);
        let mut impulses = JointImpulses::NONE;
        impulses.rod = 3.0;
        warm.push(handle(1), impulses);
        let mut cursor = 0;
        assert_eq!(warm.find(&mut cursor, Handle::from_raw(1, 1)), None);
        assert_eq!(warm.find(&mut cursor, handle(1)), Some(impulses));
        // A lower slot than the cursor is not found any more, a missing slot is not found.
        assert_eq!(warm.find(&mut cursor, handle(0)), None);
        assert_eq!(warm.find(&mut cursor, handle(5)), None);
    }

    #[test]
    fn the_warm_cache_is_bounded_hashes_nothing_when_empty_and_notices_a_bad_number() {
        let mut warm = JointWarm::new(1);
        let mut hasher = StateHasher::new();
        let empty = hasher.finish();
        warm.write_bits(&mut hasher);
        assert_eq!(hasher.finish(), empty, "an empty cache writes nothing");
        warm.push(handle(0), JointImpulses::NONE);
        warm.push(handle(1), JointImpulses::NONE);
        assert_eq!(warm.entries.len(), 1, "the capacity is one");
        warm.write_bits(&mut hasher);
        assert_ne!(hasher.finish(), empty);
        assert!(warm.is_finite());
        for index in 0..5 {
            let mut bad = JointImpulses::NONE;
            match index {
                0 => bad.point = Vec3::new(f32::NAN, 0.0, 0.0),
                1 => bad.align = Vec3::new(0.0, f32::INFINITY, 0.0),
                2 => bad.rod = f32::NAN,
                3 => bad.lower = f32::NAN,
                _ => bad.upper = f32::NAN,
            }
            assert!(!bad.is_finite(), "field {index}");
        }
    }

    #[test]
    fn every_impulse_changes_the_hash() {
        let hash = |impulses: JointImpulses| {
            let mut hasher = StateHasher::new();
            impulses.write_bits(&mut hasher);
            hasher.finish()
        };
        let base = hash(JointImpulses::NONE);
        let fields: [fn(&mut JointImpulses); 5] = [
            |i| i.point = Vec3::X,
            |i| i.align = Vec3::Y,
            |i| i.rod = 1.0,
            |i| i.lower = 1.0,
            |i| i.upper = 1.0,
        ];
        for set in fields {
            let mut changed = JointImpulses::NONE;
            set(&mut changed);
            assert_ne!(hash(changed), base);
        }
    }

    #[test]
    fn a_body_that_cannot_move_is_not_written_back_by_a_joint() {
        let mut bodies = [body(0.0, 0.0), body(1.0, 0.0)];
        bodies[1].velocity = Vec3::X;
        let mut joint = joint_of(Kind::Ball, 0.0, (Vec3::ZERO, Vec3::ZERO));
        visit(&mut bodies, &mut joint, JointPass::Solve(&sweep(false)));
        assert_eq!(bodies[0].velocity, Vec3::ZERO);
        assert!(near(bodies[1].velocity, Vec3::ZERO));
        // A joint whose body is missing does nothing.
        let mut lonely = [body(1.0, 0.0)];
        lonely[0].velocity = Vec3::X;
        visit(&mut lonely, &mut joint, JointPass::Solve(&sweep(false)));
        assert_eq!(lonely[0].velocity, Vec3::X);
    }

    fn world_body(
        world: &mut World,
        position: Vec3,
    ) -> Result<crate::d3::BodyHandle, PhysicsError> {
        world.create_body(&BodyDesc::new(
            BodyKind::Dynamic,
            position,
            Shape3::Sphere { radius: 0.5 },
        ))
    }

    #[test]
    fn a_hinge_is_prepared_from_the_world_axes_and_the_reference_angle() -> Result<(), PhysicsError>
    {
        let mut world = World::new(WorldConfig {
            max_bodies: 2,
            gravity: Vec3::ZERO,
            max_pairs: 4,
            broadphase_margin: crate::d3::DEFAULT_BROADPHASE_MARGIN,
            max_manifolds: 4,
            max_joints: 0,
            speculative_margin: crate::d3::DEFAULT_SPECULATIVE_MARGIN,
            solver: crate::d3::SolverConfig::DEFAULT,
            sleep: crate::d3::SleepConfig::DISABLED,
        })?;
        let first = world_body(&mut world, Vec3::ZERO)?;
        let second = world_body(&mut world, Vec3::new(2.0, 0.0, 0.0))?;
        let bodies = [world.body(first)?, world.body(second)?];
        let setup = JointSetup {
            handle: handle(0),
            slots: (0, 1),
            bodies,
        };
        let joint = HingeJoint3 {
            reference_angle: 0.5,
            ..HingeJoint3::new(Vec3::X, -Vec3::X, Vec3::Z, Vec3::Z)
        };
        let prepared = JointConstraint::hinge(&setup, &joint);
        assert_eq!(prepared.centres, Vec3::new(2.0, 0.0, 0.0));
        assert_eq!((prepared.anchor_a, prepared.anchor_b), (Vec3::X, -Vec3::X));
        let Kind::Hinge(terms) = prepared.kind else {
            panic!("not a hinge");
        };
        assert_eq!((terms.axis_a, terms.axis_b), (Vec3::Z, Vec3::Z));
        // The relative angle of the bodies is 0 and the reference is 0.5: the joint angle is -0.5.
        let angle = hinge_angle(terms.axis_a, terms.reference_a, terms.reference_b);
        assert!((angle + 0.5).abs() < 1e-5, "{angle}");
        let rod = JointConstraint::rod(&setup, &DistanceJoint3::new(Vec3::ZERO, Vec3::ZERO, 2.0));
        assert!(matches!(rod.kind, Kind::Rod(RodTerms { slack: false, .. })));
        let rope = DistanceJoint3 {
            limits: Some(LengthLimits { min: 1.0, max: 3.0 }),
            ..DistanceJoint3::new(Vec3::ZERO, Vec3::ZERO, 2.0)
        };
        assert!(matches!(
            JointConstraint::rod(&setup, &rope).kind,
            Kind::Rod(RodTerms { slack: true, .. })
        ));
        let ball = JointConstraint::ball(&setup, &BallJoint3::new(Vec3::ZERO, Vec3::ZERO));
        assert!(matches!(ball.kind, Kind::Ball));
        Ok(())
    }
}
