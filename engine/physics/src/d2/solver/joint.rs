//! Joints in the soft step (card 4-16): the revolute and the distance joint as soft constraints
//! in the same sub-stepping scheme as the contacts, in the manner of `Box2D` v3 **[K]**.
//!
//! *Order.* Joints are not coloured. In every pass of a sub-step (warm start, solve, relax) the
//! joints are visited first, one after the other in joint-slot order on the calling thread, and the
//! contacts follow, colour by colour. So the joint pass always sees the bodies exactly as the last
//! contact colour left them, in the reference path and in the staged one (the staged path writes
//! its bodies back after every colour), and the result does not depend on the scheduler, the
//! worker count or the lane path: joints never reach a scheduler. The price is that a scene of
//! many independent joints is solved on one thread; the follow-up is to colour joints with the
//! contacts (see `reviews/4-16.md`).
//!
//! *Softness.* The point of a hinge and a rod without a spring are held by a soft constraint of
//! `joint_hertz` and `joint_damping_ratio` (capped at a quarter of the sub-step rate, like the
//! contacts): in the solve pass the position error feeds a velocity bias, in the relax pass there
//! is no bias. A spring uses its own hertz and damping ratio in both passes. A one-sided limit
//! (angle, length) is speculative when it is not reached yet (the bodies may close the gap within
//! one sub-step and no more) and soft when it is violated.
//!
//! *Math.* Anchors are rotated into the world at the start of the step; within the step the
//! rotation each body made so far (`SolverBody::turn`) is applied to them, and the position error
//! is the relative translation plus the change of the rotated anchors, as for contacts. The hinge
//! angle is the start angle plus the relative angle of the two turns, read with `atan2` through the
//! deterministic math facade (a few times per limited joint and sub-step). Plain `f32`
//! operations in a fixed order: no fused multiply-add, no `sin`, `cos`.

use super::constraint::{Ends, Softness, SolverBody};
use super::warm::JointWarm;
use crate::core::{JointHandle, StateHasher};
use crate::d2::{AngleLimits, D2, DistanceJoint2, LengthLimits, Motor, RevoluteJoint2, Rot2};
use base::dmath;
use base::math::Vec2;
use std::f32::consts::{PI, TAU};

/// A joint axis shorter than this, in metres, is read as no axis (a rod of two anchors on top of
/// each other): the rod then pulls along nothing instead of along a direction made of noise.
const NEGLIGIBLE_LENGTH: f32 = 1e-6;

/// The impulses a joint has accumulated: what it carries from step to step as the warm start.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct JointImpulses {
    /// Hinge: the force that holds the two anchors together.
    pub point: Vec2,
    /// Rod: along the axis, spring or rigid.
    pub rod: f32,
    /// Hinge: the motor.
    pub motor: f32,
    /// The lower limit (angle or length).
    pub lower: f32,
    /// The upper limit.
    pub upper: f32,
}

impl JointImpulses {
    pub(super) fn is_finite(&self) -> bool {
        [
            self.point.x,
            self.point.y,
            self.rod,
            self.motor,
            self.lower,
            self.upper,
        ]
        .iter()
        .all(|value| value.is_finite())
    }

    pub(super) fn write_bits(&self, hasher: &mut StateHasher) {
        for value in [
            self.point.x,
            self.point.y,
            self.rod,
            self.motor,
            self.lower,
            self.upper,
        ] {
            hasher.write_f32(value);
        }
    }
}

/// How a sweep over the joints treats the soft constraint.
#[derive(Clone, Copy)]
pub(super) struct JointSweep {
    /// The sub-step length.
    pub h: f32,
    /// One over the sub-step length.
    pub inv_h: f32,
    /// True for the solve, false for the relax pass.
    pub use_bias: bool,
    /// The softness of a rigid joint.
    pub rigid: Softness,
}

impl JointSweep {
    /// The velocity bias and the two scales of a one-sided limit with `error` (negative when the
    /// limit is violated).
    fn limit_terms(&self, error: f32) -> (f32, f32, f32) {
        if error > 0.0 {
            // Speculative: the bodies may close the gap within one sub-step and no more.
            (error * self.inv_h, 1.0, 0.0)
        } else if self.use_bias {
            (
                self.rigid.bias_rate * error,
                self.rigid.mass_scale,
                self.rigid.impulse_scale,
            )
        } else {
            (0.0, 1.0, 0.0)
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

/// The rigid softness of the joints for a sub-step of `h` seconds.
pub(super) fn rigid_softness(
    hertz: f32,
    damping_ratio: f32,
    cap_fraction: f32,
    h: f32,
) -> Softness {
    Softness::new(hertz.min(cap_fraction / h), damping_ratio, h)
}

/// The angle of the hinge's second body against its first, in `(-pi, pi]`, from two rotations.
fn relative_angle(first: Rot2, second: Rot2) -> f32 {
    let (ca, sa) = first.direction();
    let (cb, sb) = second.direction();
    dmath::atan2(sb * ca - cb * sa, cb * ca + sb * sa)
}

/// `angle` brought back into `[-pi, pi]` (one turn is enough: both inputs are inside it).
fn unwound(angle: f32) -> f32 {
    if angle > PI {
        angle - TAU
    } else if angle < -PI {
        angle + TAU
    } else {
        angle
    }
}

/// What a hinge needs besides its anchors.
#[derive(Clone, Copy, Debug)]
struct HingeTerms {
    /// The joint angle at the start of the step.
    start_angle: f32,
    limits: Option<AngleLimits>,
    motor: Option<Motor>,
}

/// How a rod holds its length.
#[derive(Clone, Copy, Debug)]
enum RodMode {
    /// Held at the rest length by the joint softness.
    Rigid,
    /// Pulled toward the rest length by a spring of this softness.
    Spring(Softness),
    /// No force of its own: only the limits act (a rope or a strut).
    Slack,
}

/// What a rod needs besides its anchors.
#[derive(Clone, Copy, Debug)]
struct RodTerms {
    length: f32,
    mode: RodMode,
    limits: Option<LengthLimits>,
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    Hinge(HingeTerms),
    Rod(RodTerms),
}

/// What a joint is prepared with.
pub(super) struct JointSetup<'a> {
    pub handle: JointHandle<D2>,
    /// Slot indices of the two bodies.
    pub slots: (usize, usize),
    /// The two bodies as they are at the start of the step.
    pub bodies: [&'a crate::core::Body<D2>; 2],
    /// The length of a sub-step.
    pub h: f32,
}

/// A prepared joint.
#[derive(Clone, Copy, Debug)]
pub(super) struct JointConstraint {
    pub a: usize,
    pub b: usize,
    pub handle: JointHandle<D2>,
    /// True when a body of the joint moves this step; a joint without one is kept as it is.
    pub solved: bool,
    /// Position of the second centre minus the first at the start of the step.
    centres: Vec2,
    /// The anchors rotated into the world at the start of the step, relative to each centre.
    anchor_a: Vec2,
    anchor_b: Vec2,
    kind: Kind,
    pub impulses: JointImpulses,
}

impl JointConstraint {
    /// A hinge.
    pub(super) fn hinge(setup: &JointSetup, joint: &RevoluteJoint2) -> Self {
        let [first, second] = setup.bodies;
        let start_angle = joint.limits.map_or(0.0, |_| {
            unwound(relative_angle(first.rotation(), second.rotation()) - joint.reference_angle)
        });
        let terms = HingeTerms {
            start_angle,
            limits: joint.limits,
            motor: joint.motor,
        };
        Self::with_kind(setup, (joint.anchor_a, joint.anchor_b), Kind::Hinge(terms))
    }

    /// A rod.
    pub(super) fn rod(setup: &JointSetup, joint: &DistanceJoint2) -> Self {
        let mode = match joint.spring {
            None => RodMode::Rigid,
            Some(spring) if spring.hertz > 0.0 => {
                RodMode::Spring(Softness::new(spring.hertz, spring.damping_ratio, setup.h))
            }
            Some(_) => RodMode::Slack,
        };
        let terms = RodTerms {
            length: joint.length,
            mode,
            limits: joint.limits,
        };
        Self::with_kind(setup, (joint.anchor_a, joint.anchor_b), Kind::Rod(terms))
    }

    fn with_kind(setup: &JointSetup, local: (Vec2, Vec2), kind: Kind) -> Self {
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
            impulses: JointImpulses::default(),
        }
    }

    /// The anchors as the bodies have turned them so far in the step.
    fn arms(&self, ends: &Ends) -> (Vec2, Vec2) {
        (
            ends.a.turn.rotate(self.anchor_a),
            ends.b.turn.rotate(self.anchor_b),
        )
    }

    /// The vector from the first anchor to the second now.
    fn separation(&self, ends: &Ends, arms: (Vec2, Vec2)) -> Vec2 {
        (ends.b.shift - ends.a.shift) + (arms.1 - arms.0) + self.centres
    }

    /// Applies the impulses of the last step at the start of a sub-step.
    pub(super) fn warm_start(&self, ends: &mut Ends) {
        let arms = self.arms(ends);
        let i = self.impulses;
        match self.kind {
            Kind::Hinge(_) => {
                push_spin(ends, i.motor + i.lower - i.upper);
                push_at(ends, arms, i.point);
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
            Kind::Hinge(terms) => self.solve_hinge(&terms, ends, sweep),
            Kind::Rod(terms) => self.solve_rod(&terms, ends, sweep),
        }
    }

    fn solve_hinge(&mut self, terms: &HingeTerms, ends: &mut Ends, sweep: &JointSweep) {
        let inertia = ends.a.inv_inertia + ends.b.inv_inertia;
        let axial_mass = if inertia > 0.0 { 1.0 / inertia } else { 0.0 };
        if let Some(motor) = terms.motor {
            let speed = ends.b.spin - ends.a.spin - motor.speed;
            let most = sweep.h * motor.max_torque;
            let wanted = self.impulses.motor - axial_mass * speed;
            let total = wanted.max(-most).min(most);
            push_spin(ends, total - self.impulses.motor);
            self.impulses.motor = total;
        }
        if let Some(limits) = terms.limits {
            let angle = terms.start_angle + relative_angle(ends.a.turn, ends.b.turn);
            let lower = angle - limits.lower;
            let spin = ends.b.spin - ends.a.spin;
            let applied = one_sided(&mut self.impulses.lower, (axial_mass, lower, spin), sweep);
            push_spin(ends, applied);
            let upper = limits.upper - angle;
            let spin = ends.a.spin - ends.b.spin;
            let applied = one_sided(&mut self.impulses.upper, (axial_mass, upper, spin), sweep);
            push_spin(ends, -applied);
        }
        self.solve_point(ends, sweep);
    }

    /// Holds the two anchors together.
    fn solve_point(&mut self, ends: &mut Ends, sweep: &JointSweep) {
        let arms = self.arms(ends);
        let (ra, rb) = arms;
        let speed = ends.b.velocity_at(rb) - ends.a.velocity_at(ra);
        let (bias, mass_scale, impulse_scale) = if sweep.use_bias {
            let error = self.separation(ends, arms);
            let soft = sweep.rigid;
            (error * soft.bias_rate, soft.mass_scale, soft.impulse_scale)
        } else {
            (Vec2::ZERO, 1.0, 0.0)
        };
        let mass = ends.a.inv_mass + ends.b.inv_mass;
        let k11 = mass + ra.y * ra.y * ends.a.inv_inertia + rb.y * rb.y * ends.b.inv_inertia;
        let k12 = -ra.y * ra.x * ends.a.inv_inertia - rb.y * rb.x * ends.b.inv_inertia;
        let k22 = mass + ra.x * ra.x * ends.a.inv_inertia + rb.x * rb.x * ends.b.inv_inertia;
        let wanted = solve_symmetric((k11, k12, k22), speed + bias);
        let impulse = -(wanted * mass_scale) - self.impulses.point * impulse_scale;
        self.impulses.point += impulse;
        push_at(ends, arms, impulse);
    }

    fn solve_rod(&mut self, terms: &RodTerms, ends: &mut Ends, sweep: &JointSweep) {
        let arms = self.arms(ends);
        let (axis, length) = axis_of(self.separation(ends, arms));
        if axis == Vec2::ZERO && length.is_finite() {
            // Anchors on top of each other: there is no direction to pull along, and no impulse
            // may pile up for a direction that does not exist. A NaN length is not this case: it
            // must reach the finiteness check of the step.
            return;
        }
        let mass = rod_mass(ends, arms, axis);
        let stretch = length - terms.length;
        let (bias, mass_scale, impulse_scale) = match terms.mode {
            RodMode::Rigid if sweep.use_bias => (
                sweep.rigid.bias_rate * stretch,
                sweep.rigid.mass_scale,
                sweep.rigid.impulse_scale,
            ),
            RodMode::Rigid => (0.0, 1.0, 0.0),
            RodMode::Spring(soft) => (
                soft.bias_rate * stretch,
                soft.mass_scale,
                soft.impulse_scale,
            ),
            RodMode::Slack => (0.0, 0.0, 0.0),
        };
        let speed = closing_rate(ends, arms, axis);
        let impulse = -mass_scale * mass * (speed + bias) - impulse_scale * self.impulses.rod;
        self.impulses.rod += impulse;
        push_at(ends, arms, axis * impulse);
        if let Some(limits) = terms.limits {
            // The rate is read again before each limit: the impulses above changed it.
            let speed = closing_rate(ends, arms, axis);
            let state = (mass, length - limits.min, speed);
            let applied = one_sided(&mut self.impulses.lower, state, sweep);
            push_at(ends, arms, axis * applied);
            let speed = closing_rate(ends, arms, axis);
            let state = (mass, limits.max - length, -speed);
            let applied = one_sided(&mut self.impulses.upper, state, sweep);
            push_at(ends, arms, axis * -applied);
        }
    }
}

/// One one-sided limit: `(mass, error, speed)` is the effective mass, the distance to the limit
/// (negative when violated) and the speed at which the distance closes (negative). Accumulates into
/// `total`, which may not go below zero, and returns the impulse applied.
fn one_sided(total: &mut f32, state: (f32, f32, f32), sweep: &JointSweep) -> f32 {
    let (mass, error, speed) = state;
    let (bias, mass_scale, impulse_scale) = sweep.limit_terms(error);
    let impulse = -mass * mass_scale * (speed + bias) - impulse_scale * *total;
    let new = (*total + impulse).max(0.0);
    let applied = new - *total;
    *total = new;
    applied
}

/// The speed at which the second anchor moves away from the first along `axis`.
fn closing_rate(ends: &Ends, arms: (Vec2, Vec2), axis: Vec2) -> f32 {
    axis.dot(ends.b.velocity_at(arms.1) - ends.a.velocity_at(arms.0))
}

/// Applies `impulse` as a torque to the second body and its opposite to the first.
fn push_spin(ends: &mut Ends, impulse: f32) {
    ends.a.spin -= ends.a.inv_inertia * impulse;
    ends.b.spin += ends.b.inv_inertia * impulse;
}

/// Applies `impulse` at the second anchor and its opposite at the first.
fn push_at(ends: &mut Ends, arms: (Vec2, Vec2), impulse: Vec2) {
    ends.a.push(arms.0, -impulse);
    ends.b.push(arms.1, impulse);
}

/// The unit direction of `separation` and its length; no direction for a length that is not
/// above [`NEGLIGIBLE_LENGTH`].
fn axis_of(separation: Vec2) -> (Vec2, f32) {
    let length = separation.length();
    if length > NEGLIGIBLE_LENGTH {
        (separation * (1.0 / length), length)
    } else {
        (Vec2::ZERO, length)
    }
}

/// The effective mass of the two bodies along `axis` at the anchors; zero when nothing can move.
fn rod_mass(ends: &Ends, arms: (Vec2, Vec2), axis: Vec2) -> f32 {
    let (ca, cb) = (arms.0.cross(axis), arms.1.cross(axis));
    let k = ends.a.inv_mass
        + ends.b.inv_mass
        + ends.a.inv_inertia * ca * ca
        + ends.b.inv_inertia * cb * cb;
    if k > 0.0 { 1.0 / k } else { 0.0 }
}

/// Solves the symmetric 2 by 2 system `[k11 k12; k12 k22] x = rhs`; zero when it is singular.
fn solve_symmetric(k: (f32, f32, f32), rhs: Vec2) -> Vec2 {
    let (k11, k12, k22) = k;
    let det = k11 * k22 - k12 * k12;
    let inverse = if det == 0.0 { 0.0 } else { 1.0 / det };
    Vec2::new(
        inverse * (k22 * rhs.x - k12 * rhs.y),
        inverse * (k11 * rhs.y - k12 * rhs.x),
    )
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

/// Runs `pass` on `joint` with copies of its two bodies, which are stored back afterwards; a
/// static body is not written back (it did not change).
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
    use base::pool::Handle;

    const H: f32 = 1.0 / 240.0;

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
        let mut body = SolverBody::AT_REST;
        body.inv_mass = inv_mass;
        body.inv_inertia = inv_inertia;
        body
    }

    #[test]
    fn a_limit_not_reached_is_speculative_and_a_violated_one_is_soft() {
        // Not reached: the bodies may close the gap in one sub-step; no bias scale, no softness.
        let speculative = (0.5 * sweep(true).inv_h, 1.0, 0.0);
        assert_eq!(sweep(true).limit_terms(0.5), speculative);
        assert_eq!(sweep(false).limit_terms(0.5), speculative);
        // Violated: soft with the bias in the solve pass, rigid and bias free in the relax pass.
        assert_eq!(sweep(true).limit_terms(-0.5), (-5.0, 0.5, 0.25));
        assert_eq!(sweep(false).limit_terms(-0.5), (0.0, 1.0, 0.0));
        // Exactly at the limit counts as reached (the soft branch).
        assert_eq!(sweep(true).limit_terms(0.0), (0.0, 0.5, 0.25));
    }

    #[test]
    fn the_joint_softness_is_capped_at_a_fraction_of_the_sub_step_rate() {
        let capped = rigid_softness(1000.0, 2.0, 0.25, H);
        assert_eq!(capped, Softness::new(60.0, 2.0, H));
        let below = rigid_softness(30.0, 2.0, 0.25, H);
        assert_eq!(below, Softness::new(30.0, 2.0, H));
    }

    #[test]
    fn the_relative_angle_is_the_second_minus_the_first_and_wraps_the_short_way() {
        let angle = |a: f32, b: f32| relative_angle(Rot2::from_angle(a), Rot2::from_angle(b));
        assert!(angle(0.0, 0.0).abs() < 1e-6);
        assert!((angle(0.0, 0.5) - 0.5).abs() < 1e-5);
        assert!((angle(0.5, 0.0) + 0.5).abs() < 1e-5);
        assert!((angle(0.3, 0.9) - 0.6).abs() < 1e-5);
        // From 3.0 to -3.0 rad is a short turn forward, not a long one back.
        assert!((angle(3.0, -3.0) - (TAU - 6.0)).abs() < 1e-4);
    }

    #[test]
    fn an_angle_is_unwound_by_one_turn_into_the_half_open_range_around_zero() {
        let half = PI;
        assert_eq!(unwound(0.5), 0.5);
        assert_eq!(unwound(-0.5), -0.5);
        assert_eq!(unwound(half), half);
        assert_eq!(unwound(-half), -half);
        assert!((unwound(3.5) - (3.5 - TAU)).abs() < 1e-6);
        assert!((unwound(-3.5) - (-3.5 + TAU)).abs() < 1e-6);
    }

    #[test]
    fn the_axis_of_a_separation_is_its_direction_unless_it_is_negligible() {
        let (axis, length) = axis_of(Vec2::new(3.0, 4.0));
        assert!((axis - Vec2::new(0.6, 0.8)).length() < 1e-6);
        assert_eq!(length, 5.0);
        assert_eq!(axis_of(Vec2::ZERO), (Vec2::ZERO, 0.0));
        let (axis, length) = axis_of(Vec2::new(NEGLIGIBLE_LENGTH, 0.0));
        assert_eq!(axis, Vec2::ZERO, "exactly the limit is still negligible");
        assert_eq!(length, NEGLIGIBLE_LENGTH);
        let (axis, _) = axis_of(Vec2::new(2.0 * NEGLIGIBLE_LENGTH, 0.0));
        assert_eq!(axis, Vec2::new(1.0, 0.0));
    }

    #[test]
    fn a_symmetric_system_is_solved_and_a_singular_one_gives_zero() {
        // [2 1; 1 3] x = (1, 2): det 5, x = (0.2, 0.6).
        let x = solve_symmetric((2.0, 1.0, 3.0), Vec2::new(1.0, 2.0));
        assert!(
            (x.x - 0.2).abs() < 1e-6 && (x.y - 0.6).abs() < 1e-6,
            "{x:?}"
        );
        let diagonal = solve_symmetric((2.0, 0.0, 4.0), Vec2::new(1.0, 1.0));
        assert_eq!(diagonal, Vec2::new(0.5, 0.25));
        assert_eq!(
            solve_symmetric((1.0, 1.0, 1.0), Vec2::new(1.0, 2.0)),
            Vec2::ZERO
        );
        assert_eq!(
            solve_symmetric((0.0, 0.0, 0.0), Vec2::new(1.0, 2.0)),
            Vec2::ZERO
        );
    }

    #[test]
    fn the_effective_mass_along_an_axis_counts_both_bodies_and_their_arms() {
        let (mut a, mut b) = (body(1.0, 0.0), body(1.0, 0.0));
        let ends = Ends {
            a: &mut a,
            b: &mut b,
        };
        let axis = Vec2::new(1.0, 0.0);
        assert!((rod_mass(&ends, (Vec2::ZERO, Vec2::ZERO), axis) - 0.5).abs() < 1e-6);
        let (mut a, mut b) = (body(1.0, 2.0), body(1.0, 3.0));
        let ends = Ends {
            a: &mut a,
            b: &mut b,
        };
        // Arms 1 m off the axis (cross 1): k = 1 + 1 + 2 + 3 = 7.
        let arms = (Vec2::new(0.0, 1.0), Vec2::new(0.0, -1.0));
        assert!((rod_mass(&ends, arms, axis) - 1.0 / 7.0).abs() < 1e-6);
        let (mut a, mut b) = (body(0.0, 0.0), body(0.0, 0.0));
        let ends = Ends {
            a: &mut a,
            b: &mut b,
        };
        assert_eq!(rod_mass(&ends, (Vec2::ZERO, Vec2::ZERO), axis), 0.0);
    }

    #[test]
    fn a_one_sided_limit_accumulates_and_never_pulls() {
        let mut total = 0.0;
        // Violated by 0.1 and closing at 2 m/s the wrong way: it pushes.
        let applied = one_sided(&mut total, (2.0, 0.0, -2.0), &sweep(false));
        assert_eq!(applied, 4.0);
        assert_eq!(total, 4.0);
        // Now moving away at 1 m/s: the impulse would be negative and takes back at most the total.
        let applied = one_sided(&mut total, (2.0, 0.0, 1.0), &sweep(false));
        assert_eq!(applied, -2.0);
        assert_eq!(total, 2.0);
        let applied = one_sided(&mut total, (2.0, 0.0, 10.0), &sweep(false));
        assert_eq!(applied, -2.0, "it stops at zero");
        assert_eq!(total, 0.0);
    }

    #[test]
    fn a_violated_soft_limit_scales_the_mass_and_gives_back_part_of_the_total() {
        // mass 2, violated by 0.5, closing at 2 m/s, with 1 already accumulated; the soft terms of
        // the solve pass are (bias -5, mass scale 0.5, impulse scale 0.25):
        // impulse = -2 * 0.5 * (-2 - 5) - 0.25 * 1 = 6.75.
        let mut total = 1.0;
        let applied = one_sided(&mut total, (2.0, -0.5, -2.0), &sweep(true));
        assert_eq!(applied, 6.75);
        assert_eq!(total, 7.75);
    }

    fn hinge(
        a_spin: f32,
        b_spin: f32,
        inertia: f32,
        terms: HingeTerms,
    ) -> (JointConstraint, [SolverBody; 2]) {
        let mut joint = constraint(0, true, 0.0);
        joint.kind = Kind::Hinge(terms);
        let (mut a, mut b) = (body(0.0, inertia), body(0.0, inertia));
        a.spin = a_spin;
        b.spin = b_spin;
        (joint, [a, b])
    }

    fn solve_hinge_once(joint: &mut JointConstraint, bodies: &mut [SolverBody; 2]) {
        let [a, b] = bodies;
        joint.solve(&mut Ends { a, b }, &sweep(true));
    }

    fn limited(lower: f32, upper: f32) -> HingeTerms {
        HingeTerms {
            start_angle: 0.0,
            limits: Some(AngleLimits { lower, upper }),
            motor: None,
        }
    }

    #[test]
    fn a_motor_reads_the_relative_spin_of_both_bodies() {
        let motor = Some(Motor {
            max_torque: 100.0,
            speed: 0.0,
        });
        let terms = HingeTerms {
            start_angle: 0.0,
            limits: None,
            motor,
        };
        // Both turn at 1 rad/s: no relative spin, so the motor at speed 0 does nothing.
        let (mut joint, mut bodies) = hinge(1.0, 1.0, 1.0, terms);
        solve_hinge_once(&mut joint, &mut bodies);
        assert_eq!(joint.impulses.motor, 0.0);
        assert_eq!((bodies[0].spin, bodies[1].spin), (1.0, 1.0));
    }

    #[test]
    fn a_motor_between_bodies_that_cannot_turn_applies_nothing() {
        let terms = HingeTerms {
            start_angle: 0.0,
            limits: None,
            motor: Some(Motor {
                max_torque: 100.0,
                speed: 1.0,
            }),
        };
        let (mut joint, mut bodies) = hinge(0.0, 0.0, 0.0, terms);
        solve_hinge_once(&mut joint, &mut bodies);
        assert_eq!(joint.impulses.motor, 0.0, "no inertia, no axial mass");
    }

    #[test]
    fn a_limit_reads_the_relative_spin_of_both_bodies_on_each_side() {
        // Violated lower limit (the angle is 0, the limit 0.5), both bodies turning at 1 rad/s:
        // impulse = -0.5 * 0.5 * (0 - 5) = 1.25 with the soft terms of the solve pass.
        let (mut joint, mut bodies) = hinge(1.0, 1.0, 1.0, limited(0.5, 1.0));
        solve_hinge_once(&mut joint, &mut bodies);
        assert_eq!(joint.impulses.lower, 1.25);
        // The same for a violated upper limit.
        let (mut joint, mut bodies) = hinge(1.0, 1.0, 1.0, limited(-1.0, -0.5));
        solve_hinge_once(&mut joint, &mut bodies);
        assert_eq!(joint.impulses.upper, 1.25);
    }

    #[test]
    fn a_rigid_rod_uses_the_softness_with_bias_and_none_in_the_relax_pass() {
        // The rod has length 1 and the bodies are 1.5 m apart: 0.5 m too long.
        let stretched = |use_bias: bool| {
            let mut joint = constraint(0, true, 0.0);
            joint.centres = Vec2::new(1.5, 0.0);
            let (mut a, mut b) = (body(0.0, 0.0), body(1.0, 0.0));
            joint.solve(
                &mut Ends {
                    a: &mut a,
                    b: &mut b,
                },
                &sweep(use_bias),
            );
            b.velocity
        };
        // Solve pass: impulse = -0.5 * 1 * (10 * 0.5) = -2.5, pushing the second body back.
        assert_eq!(stretched(true), Vec2::new(-2.5, 0.0));
        // Relax pass: no bias, so a body at rest stays at rest.
        assert_eq!(stretched(false), Vec2::ZERO);
    }

    #[test]
    fn a_spin_impulse_turns_the_second_body_one_way_and_the_first_the_other() {
        let (mut a, mut b) = (body(0.0, 0.5), body(0.0, 0.25));
        let mut ends = Ends {
            a: &mut a,
            b: &mut b,
        };
        push_spin(&mut ends, 2.0);
        assert_eq!((a.spin, b.spin), (-1.0, 0.5));
    }

    fn handle(index: u32) -> JointHandle<D2> {
        Handle::from_raw(index, 0)
    }

    fn constraint(index: u32, solved: bool, rod: f32) -> JointConstraint {
        JointConstraint {
            a: 0,
            b: 1,
            handle: handle(index),
            solved,
            centres: Vec2::ZERO,
            anchor_a: Vec2::ZERO,
            anchor_b: Vec2::ZERO,
            kind: Kind::Rod(RodTerms {
                length: 1.0,
                mode: RodMode::Rigid,
                limits: None,
            }),
            impulses: JointImpulses {
                rod,
                ..JointImpulses::default()
            },
        }
    }

    #[test]
    fn a_set_keeps_the_impulses_of_the_last_step_for_the_same_joint() {
        let mut set = JointSet::new(4);
        set.add(constraint(0, true, 1.0));
        set.add(constraint(2, false, 2.0));
        assert_eq!(set.counts(), (1, 1));
        set.finish();
        assert!(set.is_finite());
        set.commit();
        set.clear();
        // Joint 1 is new, joint 2 is back: it finds its impulse, the new one starts at zero.
        set.add(constraint(1, true, 0.0));
        set.add(constraint(2, true, 0.0));
        set.add(constraint(0, true, 0.0));
        let rods: Vec<f32> = set
            .constraints
            .iter()
            .map(|joint| joint.impulses.rod)
            .collect();
        assert_eq!(rods, vec![0.0, 2.0, 0.0]);
    }

    #[test]
    fn a_limit_sees_the_speed_left_by_the_rod_impulse_before_it() {
        // A rigid rod of length 1 stretched to 1.5 m with its upper limit at 1 m. The rod pulls the
        // second body back with 0.5 * 1 * (10 * 0.5) = 2.5, so it moves at -2.5 (closing). The
        // upper limit is violated by 0.5 (soft terms: bias -5, mass scale 0.5), and reads the
        // closing speed again: -0.5 * (2.5 - 5) = 1.25 more, giving -3.75. With the speed from
        // before the rod impulse it would be -0.5 * (0 - 5) = 2.5, giving -5.0.
        let mut joint = constraint(0, true, 0.0);
        joint.centres = Vec2::new(1.5, 0.0);
        joint.kind = Kind::Rod(RodTerms {
            length: 1.0,
            mode: RodMode::Rigid,
            limits: Some(LengthLimits { min: 0.5, max: 1.0 }),
        });
        let (mut a, mut b) = (body(0.0, 0.0), body(1.0, 0.0));
        joint.solve(
            &mut Ends {
                a: &mut a,
                b: &mut b,
            },
            &sweep(true),
        );
        assert_eq!(b.velocity, Vec2::new(-3.75, 0.0));
        assert_eq!(joint.impulses.upper, 1.25);
    }

    #[test]
    fn a_reused_slot_does_not_inherit_the_impulses_of_the_joint_before_it() {
        let mut set = JointSet::new(2);
        set.add(constraint(0, true, 5.0));
        set.finish();
        set.commit();
        set.clear();
        let mut reused = constraint(0, true, 0.0);
        reused.handle = Handle::from_raw(0, 1);
        set.add(reused);
        assert_eq!(set.constraints[0].impulses.rod, 0.0);
    }

    #[test]
    fn a_set_holds_at_most_its_capacity_and_reports_non_finite_impulses() {
        let mut set = JointSet::new(1);
        set.add(constraint(0, true, f32::NAN));
        set.add(constraint(1, true, 0.0));
        assert_eq!(set.counts(), (1, 0));
        set.finish();
        assert!(!set.is_finite());
    }

    #[test]
    fn only_solved_joints_move_the_bodies_and_a_static_body_is_not_written_back() {
        let mut frozen = constraint(0, false, 3.0);
        frozen.centres = Vec2::new(1.0, 0.0);
        let mut set = JointSet::new(2);
        set.add(frozen);
        let mut moving = body(1.0, 0.0);
        moving.velocity = Vec2::new(0.0, 1.0);
        let mut bodies = [moving, body(0.0, 0.0)];
        let before = bodies;
        set.sweep(&mut bodies, JointPass::WarmStart);
        assert_eq!(bodies[0].velocity, before[0].velocity);
        let mut live = constraint(0, true, 3.0);
        live.centres = Vec2::new(1.0, 0.0);
        let mut set = JointSet::new(2);
        set.add(live);
        set.sweep(&mut bodies, JointPass::WarmStart);
        // The axis points from the first body to the second; the impulse pushes the first back.
        assert_eq!(bodies[0].velocity, Vec2::new(-3.0, 1.0));
        assert_eq!(
            bodies[1].velocity,
            Vec2::ZERO,
            "a static end is never written"
        );
    }
}
