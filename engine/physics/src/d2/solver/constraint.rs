//! One contact constraint and the math of the soft step: the soft constraint coefficients, a body as
//! the solver sees it, a manifold prepared once per step, and the three solves (normal, friction,
//! restitution). Plain `f32` operations in a fixed order: no fused multiply-add, no `sin`, `cos`.
//!
//! The separation of a point is not recomputed from the shapes: it is the manifold's separation
//! plus the relative motion of the two anchors since the start of the step (the translation of both
//! bodies and the rotation of the anchors by the turn each body made), which is what lets the
//! manifold be made once per step.

mod lanes;

pub(super) use crate::core::soft::{
    HERTZ_LIMIT_FRACTION, STATIC_STIFFNESS_FACTOR, Softness, Sweep, inverse_or_zero, limited,
};

use super::warm::{CachedManifold, CachedPoint, Impulse};
use crate::core::{Body, BodyHandle, damping_factor};
use crate::d2::{ContactPoint2, D2, FeatureId, MAX_MANIFOLD_POINTS, Manifold2, Rot2};
use base::math::Vec2;

/// What the solver needs of the step to set a body up.
#[derive(Clone, Copy)]
pub(super) struct StepTerms {
    pub gravity: Vec2,
    /// Length of a sub-step.
    pub h: f32,
}

/// A body as the solver works on it: velocities, the motion since the start of the step, and the
/// terms that stay the same over the sub-steps.
#[derive(Clone, Copy, Debug)]
pub(super) struct SolverBody {
    pub velocity: Vec2,
    pub spin: f32,
    /// Translation since the start of the step.
    pub shift: Vec2,
    /// Rotation since the start of the step.
    pub turn: Rot2,
    pub inv_mass: f32,
    pub inv_inertia: f32,
    accel: Vec2,
    spin_accel: f32,
    linear_factor: f32,
    angular_factor: f32,
}

impl SolverBody {
    pub(super) const AT_REST: Self = Self {
        velocity: Vec2::ZERO,
        spin: 0.0,
        shift: Vec2::ZERO,
        turn: Rot2::IDENTITY,
        inv_mass: 0.0,
        inv_inertia: 0.0,
        accel: Vec2::ZERO,
        spin_accel: 0.0,
        linear_factor: 1.0,
        angular_factor: 1.0,
    };

    pub(super) fn new(body: &Body<D2>, terms: StepTerms) -> Self {
        let inv_mass = body.inverse_mass();
        let inv_inertia = body.inverse_inertia();
        Self {
            velocity: body.linear_velocity(),
            spin: body.angular_velocity(),
            inv_mass,
            inv_inertia,
            accel: terms.gravity + body.force() * inv_mass,
            spin_accel: inv_inertia * body.torque(),
            linear_factor: damping_factor(body.linear_damping(), terms.h),
            angular_factor: damping_factor(body.angular_damping(), terms.h),
            ..Self::AT_REST
        }
    }

    /// Gravity, forces and damping over `h` seconds (semi-implicit Euler, as `Body::integrate`).
    pub(super) fn integrate_velocity(&mut self, h: f32) {
        self.velocity = (self.velocity + self.accel * h) * self.linear_factor;
        self.spin = (self.spin + self.spin_accel * h) * self.angular_factor;
    }

    /// Moves the body by its velocities over `h` seconds.
    pub(super) fn integrate_position(&mut self, h: f32) {
        self.shift += self.velocity * h;
        self.turn = self.turn.advanced(self.spin * h);
    }

    /// The velocity of the point `anchor` away from the centre.
    pub(super) fn velocity_at(&self, anchor: Vec2) -> Vec2 {
        Vec2::new(
            self.velocity.x - self.spin * anchor.y,
            self.velocity.y + self.spin * anchor.x,
        )
    }

    /// Applies `impulse` at the point `anchor` away from the centre.
    pub(super) fn push(&mut self, anchor: Vec2, impulse: Vec2) {
        self.velocity += impulse * self.inv_mass;
        self.spin += self.inv_inertia * anchor.cross(impulse);
    }

    pub(super) fn is_static(&self) -> bool {
        self.inv_mass == 0.0
    }
}

/// Which bodies a constraint joins: their handles and their slot indices.
#[derive(Clone, Copy)]
pub(super) struct Link {
    pub first: BodyHandle<D2>,
    pub second: BodyHandle<D2>,
    pub a: usize,
    pub b: usize,
}

/// The mixed material of the two bodies of a contact.
#[derive(Clone, Copy)]
pub(super) struct Surface {
    pub friction: f32,
    pub restitution: f32,
}

/// What a manifold is prepared with.
pub(super) struct Setup<'a> {
    pub bodies: [&'a SolverBody; 2],
    pub surface: Surface,
    pub softness: Softness,
}

/// One point of a prepared manifold.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Point {
    anchor_a: Vec2,
    anchor_b: Vec2,
    /// The separation with the anchors' own offset taken out, so that adding the relative motion of
    /// the anchors gives the separation at any time in the step.
    base_separation: f32,
    normal_mass: f32,
    tangent_mass: f32,
    /// The approach speed along the normal before the solve, for restitution.
    relative_velocity: f32,
    normal_impulse: f32,
    tangent_impulse: f32,
    max_normal_impulse: f32,
    id: FeatureId,
}

/// The right-hand perpendicular of the normal: the direction friction acts in.
fn tangent_of(normal: Vec2) -> Vec2 {
    Vec2::new(normal.y, -normal.x)
}

impl Point {
    fn new(source: &ContactPoint2, normal: Vec2, bodies: [&SolverBody; 2]) -> Self {
        let [a, b] = bodies;
        let (ra, rb) = (source.anchor_a, source.anchor_b);
        let effective_mass = |direction: Vec2| {
            let (rna, rnb) = (ra.cross(direction), rb.cross(direction));
            inverse_or_zero(
                a.inv_mass + b.inv_mass + a.inv_inertia * rna * rna + b.inv_inertia * rnb * rnb,
            )
        };
        Self {
            anchor_a: ra,
            anchor_b: rb,
            base_separation: source.separation - (rb - ra).dot(normal),
            normal_mass: effective_mass(normal),
            tangent_mass: effective_mass(tangent_of(normal)),
            relative_velocity: normal.dot(b.velocity_at(rb) - a.velocity_at(ra)),
            id: source.id,
            ..Self::default()
        }
    }

    fn impulse(&self) -> Impulse {
        Impulse {
            normal: self.normal_impulse,
            tangent: self.tangent_impulse,
        }
    }
}

/// The two bodies of a constraint, copied out of the solver's array and written back after.
pub(super) struct Ends<'a> {
    pub a: &'a mut SolverBody,
    pub b: &'a mut SolverBody,
}

/// The passes over the constraints of a sub-step that move velocities.
#[derive(Clone, Copy)]
pub(super) enum Pass<'a> {
    /// Applies the impulses of the last step.
    WarmStart,
    /// One soft solve (the sweep says whether it has the bias).
    Solve(&'a Sweep),
    /// Bounce, once after the last sub-step.
    Restitute(&'a Sweep),
}

impl Pass<'_> {
    /// Runs the pass on one constraint and its two bodies: the scalar reference.
    pub(super) fn apply(self, constraint: &mut Constraint, ends: &mut Ends) {
        match self {
            Self::WarmStart => constraint.warm_start(ends),
            Self::Solve(sweep) => constraint.solve(ends, sweep),
            Self::Restitute(sweep) => constraint.restitute(ends, sweep),
        }
    }
}

/// A constraint together with copies of its two bodies: what a chunk of a colour works on. The
/// bodies are read when the chunk starts and stored back, after the whole colour has run, in the
/// order of the reference.
#[derive(Clone, Copy)]
pub(super) struct Staged {
    pub constraint: Constraint,
    pub a: SolverBody,
    pub b: SolverBody,
}

impl Staged {
    /// A constraint with placeholder bodies, to be replaced before the first use.
    pub(super) fn new(constraint: Constraint) -> Self {
        Self {
            constraint,
            a: SolverBody::AT_REST,
            b: SolverBody::AT_REST,
        }
    }

    /// The scalar reference on the copies.
    pub(super) fn run(&mut self, pass: Pass) {
        let mut ends = Ends {
            a: &mut self.a,
            b: &mut self.b,
        };
        pass.apply(&mut self.constraint, &mut ends);
    }
}

/// How many constraints the lane path solves at once.
pub(super) use lanes::LANES;

/// Runs `pass` on four staged constraints with lanes; bit for bit what four calls of
/// [`Staged::run`] give.
pub(super) fn run_lanes(group: &mut [Staged; LANES], pass: Pass) {
    lanes::run(group, pass);
}

/// A prepared manifold: the contact between two bodies.
#[derive(Clone, Copy, Debug)]
pub(super) struct Constraint {
    pub a: usize,
    pub b: usize,
    first: BodyHandle<D2>,
    second: BodyHandle<D2>,
    normal: Vec2,
    friction: f32,
    restitution: f32,
    softness: Softness,
    count: usize,
    points: [Point; MAX_MANIFOLD_POINTS],
}

impl Constraint {
    pub(super) fn new(link: Link, manifold: &Manifold2, setup: &Setup) -> Self {
        let mut points = [Point::default(); MAX_MANIFOLD_POINTS];
        let sources = manifold.points();
        for (point, source) in points.iter_mut().zip(sources) {
            *point = Point::new(source, manifold.normal, setup.bodies);
        }
        Self {
            a: link.a,
            b: link.b,
            first: link.first,
            second: link.second,
            normal: manifold.normal,
            friction: setup.surface.friction,
            restitution: setup.surface.restitution,
            softness: setup.softness,
            count: sources.len().min(MAX_MANIFOLD_POINTS),
            points,
        }
    }

    /// Takes over the impulses of the same features from the last step.
    pub(super) fn warm_from(&mut self, cached: &CachedManifold) {
        for point in self.points.iter_mut().take(self.count) {
            let impulse = cached.impulse_of(point.id);
            point.normal_impulse = impulse.normal;
            point.tangent_impulse = impulse.tangent;
        }
    }

    /// What is kept for the next step.
    pub(super) fn cached(&self) -> CachedManifold {
        let mut kept = [CachedPoint::default(); MAX_MANIFOLD_POINTS];
        for (slot, point) in kept.iter_mut().zip(&self.points).take(self.count) {
            *slot = CachedPoint {
                id: point.id,
                impulse: point.impulse(),
            };
        }
        CachedManifold::new(
            self.first,
            self.second,
            kept.get(..self.count).unwrap_or(&[]),
        )
    }

    /// Applies the impulses of the last step at the start of a sub-step.
    pub(super) fn warm_start(&self, ends: &mut Ends) {
        let tangent = tangent_of(self.normal);
        for point in self.points.iter().take(self.count) {
            let impulse = self.normal * point.normal_impulse + tangent * point.tangent_impulse;
            ends.a.push(point.anchor_a, -impulse);
            ends.b.push(point.anchor_b, impulse);
        }
    }

    /// One sweep: friction first, then the normal, which is the more important one and so gets the
    /// last word.
    pub(super) fn solve(&mut self, ends: &mut Ends, sweep: &Sweep) {
        let tangent = tangent_of(self.normal);
        let limit = self.friction;
        for point in self.points.iter_mut().take(self.count) {
            let relative = ends.b.velocity_at(point.anchor_b) - ends.a.velocity_at(point.anchor_a);
            let delta = -point.tangent_mass * relative.dot(tangent);
            let bound = limit * point.normal_impulse;
            let applied = limited(point.tangent_impulse + delta, bound) - point.tangent_impulse;
            point.tangent_impulse += applied;
            let impulse = tangent * applied;
            ends.a.push(point.anchor_a, -impulse);
            ends.b.push(point.anchor_b, impulse);
        }
        for point in self.points.iter_mut().take(self.count) {
            let moved = (ends.b.shift - ends.a.shift)
                + (ends.b.turn.rotate(point.anchor_b) - ends.a.turn.rotate(point.anchor_a));
            let separation = moved.dot(self.normal) + point.base_separation;
            let (bias, mass_scale, impulse_scale) = sweep.terms(separation, &self.softness);
            let relative = ends.b.velocity_at(point.anchor_b) - ends.a.velocity_at(point.anchor_a);
            let delta = -point.normal_mass * mass_scale * (self.normal.dot(relative) + bias)
                - impulse_scale * point.normal_impulse;
            let applied = Self::accumulate(point, delta);
            let impulse = self.normal * applied;
            ends.a.push(point.anchor_a, -impulse);
            ends.b.push(point.anchor_b, impulse);
        }
    }

    /// Adds `delta` to the accumulated normal impulse of `point`, which may not go below zero (a
    /// contact pushes, never pulls); returns the impulse that was applied.
    fn accumulate(point: &mut Point, delta: f32) -> f32 {
        let total = (point.normal_impulse + delta).max(0.0);
        let applied = total - point.normal_impulse;
        point.normal_impulse = total;
        point.max_normal_impulse = point.max_normal_impulse.max(total);
        applied
    }

    /// Bounce: after the last sub-step, a point that approached faster than the threshold and
    /// pushed at all gets the speed `restitution` times its approach speed away from the other body.
    pub(super) fn restitute(&mut self, ends: &mut Ends, sweep: &Sweep) {
        if self.restitution == 0.0 {
            return;
        }
        let approach = -sweep.restitution_threshold;
        let bounces = |point: &&mut Point| {
            point.relative_velocity <= approach && point.max_normal_impulse != 0.0
        };
        for point in self.points.iter_mut().take(self.count).filter(bounces) {
            let relative = ends.b.velocity_at(point.anchor_b) - ends.a.velocity_at(point.anchor_a);
            let delta = -point.normal_mass
                * (self.normal.dot(relative) + self.restitution * point.relative_velocity);
            let applied = Self::accumulate(point, delta);
            let impulse = self.normal * applied;
            ends.a.push(point.anchor_a, -impulse);
            ends.b.push(point.anchor_b, impulse);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tangent_is_the_normal_turned_a_quarter_clockwise() {
        assert_eq!(tangent_of(Vec2::new(0.0, 1.0)), Vec2::new(1.0, 0.0));
        assert_eq!(tangent_of(Vec2::new(1.0, 0.0)), Vec2::new(0.0, -1.0));
    }
}
