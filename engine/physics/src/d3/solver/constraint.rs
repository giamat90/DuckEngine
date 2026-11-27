//! One contact constraint of the 3D solver: a manifold prepared once per step, and the three solves
//! (friction on two tangents, normal, restitution). The tangent impulses are limited to a circular
//! friction cone. Plain `f32` operations in a fixed order: no fused multiply-add, no `sin`, `cos`.
//!
//! The separation of a point is not recomputed from the shapes: it is the manifold's separation
//! plus the relative motion of the two anchors since the start of the step (the translation of both
//! bodies and the rotation of the anchors by the turn each body made).

use super::body::SolverBody;
use super::warm::{CachedManifold, CachedPoint};
use crate::core::BodyHandle;
use crate::core::soft::{Softness, Sweep, inverse_or_zero};
use crate::d3::{ContactPoint3, D3, MAX_MANIFOLD_POINTS, Manifold3};
use base::math::Vec3;

/// `1 / sqrt(3)`: a normal with a component at least this large in x is turned away from the x axis
/// to find a first tangent, so that the tangent is never nearly zero.
const INV_SQRT_3: f32 = 0.577_350_3;

/// Two unit vectors that, with `normal`, make a right-handed frame. They are a function of the
/// normal alone, so the same normal gives the same tangents on every run.
pub(super) fn tangents_of(normal: Vec3) -> [Vec3; 2] {
    let across = if normal.x.abs() >= INV_SQRT_3 {
        Vec3::new(normal.y, -normal.x, 0.0)
    } else {
        Vec3::new(0.0, normal.z, -normal.y)
    };
    let first = across.normalized().unwrap_or(Vec3::X);
    [first, normal.cross(first)]
}

/// The two tangent components of `total` limited to a disc of radius `bound`; zero when the bound is
/// not a number or negative.
fn within_cone(total: [f32; 2], bound: f32) -> [f32; 2] {
    if bound >= 0.0 {
        cone_limited(total, bound)
    } else {
        [0.0, 0.0]
    }
}

/// `total` shrunk to the disc of radius `bound`, which is zero or more.
fn cone_limited(total: [f32; 2], bound: f32) -> [f32; 2] {
    let length_squared = total[0] * total[0] + total[1] * total[1];
    if length_squared > bound * bound {
        let scale = bound / length_squared.sqrt();
        [total[0] * scale, total[1] * scale]
    } else {
        total
    }
}

/// Which bodies a constraint joins: their handles and their slot indices.
#[derive(Clone, Copy)]
pub(super) struct Link {
    pub first: BodyHandle<D3>,
    pub second: BodyHandle<D3>,
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
    /// Where the two bodies' centres of mass are.
    pub centres: [Vec3; 2],
    pub surface: Surface,
    pub softness: Softness,
}

/// The normal and the two tangents of a manifold.
#[derive(Clone, Copy)]
struct Frame {
    normal: Vec3,
    tangents: [Vec3; 2],
}

/// One point of a prepared manifold.
#[derive(Clone, Copy, Debug, Default)]
struct Point {
    /// Where the point was in world space when the manifold was made.
    at: Vec3,
    anchor_a: Vec3,
    anchor_b: Vec3,
    /// The separation with the anchors' own offset taken out, so that adding the relative motion of
    /// the anchors gives the separation at any time in the step.
    base_separation: f32,
    normal_mass: f32,
    tangent_mass: [f32; 2],
    /// The approach speed along the normal before the solve, for restitution.
    relative_velocity: f32,
}

/// What a solve changes at one point: the impulses accumulated so far.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Impulse {
    normal: f32,
    tangent: [f32; 2],
    /// The largest normal impulse of the step, for restitution.
    max_normal: f32,
}

/// The impulses of the points of one constraint.
pub(super) type Impulses = [Impulse; MAX_MANIFOLD_POINTS];

impl Point {
    fn new(source: &ContactPoint3, frame: &Frame, setup: &Setup) -> Self {
        let [body_a, body_b] = setup.bodies;
        let (ra, rb) = (
            source.point - setup.centres[0],
            source.point - setup.centres[1],
        );
        let effective_mass = |direction: Vec3| {
            let (turn_a, turn_b) = (ra.cross(direction), rb.cross(direction));
            inverse_or_zero(
                body_a.inv_mass
                    + body_b.inv_mass
                    + turn_a.dot(body_a.inv_inertia * turn_a)
                    + turn_b.dot(body_b.inv_inertia * turn_b),
            )
        };
        Self {
            at: source.point,
            anchor_a: ra,
            anchor_b: rb,
            base_separation: source.separation - (rb - ra).dot(frame.normal),
            normal_mass: effective_mass(frame.normal),
            tangent_mass: frame.tangents.map(effective_mass),
            relative_velocity: frame
                .normal
                .dot(body_b.velocity_at(rb) - body_a.velocity_at(ra)),
        }
    }

    /// The impulses of the kept point that is where this point is, if there is one.
    fn warmed_from(
        &self,
        cached: &CachedManifold,
        tangents: &[Vec3; 2],
        taken: &mut u8,
    ) -> Impulse {
        let mut impulse = Impulse::default();
        if let Some(kept) = cached.take_near(self.at, taken) {
            impulse.normal = kept.normal;
            impulse.tangent = tangents.map(|tangent| kept.tangent.dot(tangent));
        }
        impulse
    }
}

impl Impulse {
    /// Adds `delta` to the accumulated normal impulse, which may not go below zero (a contact
    /// pushes, never pulls); returns the impulse that was applied.
    fn accumulate(&mut self, delta: f32) -> f32 {
        let total = (self.normal + delta).max(0.0);
        let applied = total - self.normal;
        self.normal = total;
        self.max_normal = self.max_normal.max(total);
        applied
    }

    /// The impulse across the normal as a vector in world axes.
    fn tangent_vector(&self, tangents: &[Vec3; 2]) -> Vec3 {
        tangents[0] * self.tangent[0] + tangents[1] * self.tangent[1]
    }
}

/// The two bodies of a constraint, copied out of the solver's array and written back after.
pub(super) struct Ends<'a> {
    pub a: &'a mut SolverBody,
    pub b: &'a mut SolverBody,
}

impl Ends<'_> {
    /// The velocity of the second body's point relative to the first body's point.
    fn relative_velocity(&self, point: &Point) -> Vec3 {
        self.b.velocity_at(point.anchor_b) - self.a.velocity_at(point.anchor_a)
    }

    /// Pushes the second body by `impulse` and the first by the opposite.
    fn push_apart(&mut self, point: &Point, impulse: Vec3) {
        self.a.push(point.anchor_a, -impulse);
        self.b.push(point.anchor_b, impulse);
    }
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

impl<'a> Pass<'a> {
    /// The sweep of a pass that can be run on two constraints side by side: the soft solve.
    pub(super) fn pair_sweep(self) -> Option<&'a Sweep> {
        match self {
            Self::Solve(sweep) => Some(sweep),
            Self::WarmStart | Self::Restitute(_) => None,
        }
    }
}

impl Pass<'_> {
    /// Runs the pass on one constraint and its two bodies.
    pub(super) fn apply(self, constraint: &mut Constraint, ends: &mut Ends) {
        self.apply_to(&constraint.shape, &mut constraint.impulses, ends);
    }

    /// Runs the pass on the shape of a constraint and its impulses (the staged solve keeps the
    /// two apart: the shape is shared and read only, the impulses are copied).
    pub(super) fn apply_to(self, shape: &Shape, impulses: &mut Impulses, ends: &mut Ends) {
        match self {
            Self::WarmStart => shape.warm_start(impulses, ends),
            Self::Solve(sweep) => shape.solve(impulses, ends, sweep),
            Self::Restitute(sweep) => shape.restitute(impulses, ends, sweep),
        }
    }
}

/// The impulses of a constraint with copies of its two bodies: what a chunk of the staged colour
/// solve works on (`stage`). The copies are read when the chunk starts and stored back, after the
/// whole colour has run, in the order of the reference.
#[derive(Clone, Copy)]
pub(super) struct Staged {
    pub impulses: Impulses,
    pub a: SolverBody,
    pub b: SolverBody,
}

impl Staged {
    /// The scalar reference on the copies.
    pub(super) fn run(&mut self, shape: &Shape, pass: Pass) {
        let mut ends = Ends {
            a: &mut self.a,
            b: &mut self.b,
        };
        pass.apply_to(shape, &mut self.impulses, &mut ends);
    }
}

/// One constraint of a pair that is solved side by side: its shape, its impulses and the copies of
/// its two bodies.
pub(super) struct Side<'a> {
    pub shape: &'a Shape,
    pub impulses: &'a mut Impulses,
    pub ends: Ends<'a>,
}

impl Staged {
    /// This staged constraint as one side of a pair, on `shape`.
    pub(super) fn side<'a>(&'a mut self, shape: &'a Shape) -> Side<'a> {
        Side {
            shape,
            impulses: &mut self.impulses,
            ends: Ends {
                a: &mut self.a,
                b: &mut self.b,
            },
        }
    }
}

/// The soft solve of two constraints that share no moving body, point by point side by side: the
/// first point of one, the first point of the other, the second of one and so on, friction for
/// all the points and then the normal for all. Every operation of one constraint is the same, in
/// the same order, as when it is solved alone (`Shape::solve`), so the results are bit for bit
/// those of solving the two one after the other; the two chains of dependent operations are what
/// the processor can run at the same time (card 8-97: about a fifth less of a step of a large
/// pyramid).
pub(super) fn solve_pair(first: &mut Side, second: &mut Side, sweep: &Sweep) {
    let most = first.shape.count.max(second.shape.count);
    for index in 0..most {
        first.friction_at(index);
        second.friction_at(index);
    }
    for index in 0..most {
        first.normal_at(index, sweep);
        second.normal_at(index, sweep);
    }
}

impl Side<'_> {
    /// The friction solve of point `index`, if the constraint has one.
    #[inline]
    fn friction_at(&mut self, index: usize) {
        let live = index < self.shape.count;
        if let (Some(point), Some(accumulated), true) = (
            self.shape.points.get(index),
            self.impulses.get_mut(index),
            live,
        ) {
            self.shape.friction_at(point, accumulated, &mut self.ends);
        }
    }

    /// The normal solve of point `index`, if the constraint has one.
    #[inline]
    fn normal_at(&mut self, index: usize, sweep: &Sweep) {
        let live = index < self.shape.count;
        if let (Some(point), Some(accumulated), true) = (
            self.shape.points.get(index),
            self.impulses.get_mut(index),
            live,
        ) {
            self.shape
                .normal_at(point, accumulated, &mut self.ends, sweep);
        }
    }
}

/// A prepared manifold: the contact between two bodies, with the impulses it has accumulated.
#[derive(Clone, Copy, Debug)]
pub(super) struct Constraint {
    pub shape: Shape,
    pub impulses: Impulses,
}

impl Constraint {
    pub(super) fn new(link: Link, manifold: &Manifold3, setup: &Setup) -> Self {
        Self {
            shape: Shape::new(link, manifold, setup),
            impulses: Impulses::default(),
        }
    }

    /// Takes over the impulses of the points of the last step that are where these points are.
    pub(super) fn warm_from(&mut self, cached: &CachedManifold) {
        self.impulses = self.shape.warmed_from(cached);
    }

    /// The softness this constraint was made with.
    #[cfg(test)]
    pub(super) fn softness(&self) -> Softness {
        self.shape.softness
    }

    /// What is kept for the next step.
    pub(super) fn cached(&self) -> CachedManifold {
        self.shape.cached(&self.impulses)
    }

    #[cfg(test)]
    pub(super) fn solve(&mut self, ends: &mut Ends, sweep: &Sweep) {
        self.shape.solve(&mut self.impulses, ends, sweep);
    }

    #[cfg(test)]
    pub(super) fn restitute(&mut self, ends: &mut Ends, sweep: &Sweep) {
        self.shape.restitute(&mut self.impulses, ends, sweep);
    }
}

/// What a pass reads of a constraint and never changes: the bodies it joins, the frame, the
/// material and the points as prepared.
#[derive(Clone, Copy, Debug)]
pub(super) struct Shape {
    pub a: usize,
    pub b: usize,
    first: BodyHandle<D3>,
    second: BodyHandle<D3>,
    normal: Vec3,
    tangents: [Vec3; 2],
    friction: f32,
    restitution: f32,
    softness: Softness,
    count: usize,
    points: [Point; MAX_MANIFOLD_POINTS],
}

impl Shape {
    fn new(link: Link, manifold: &Manifold3, setup: &Setup) -> Self {
        let frame = Frame {
            normal: manifold.normal,
            tangents: tangents_of(manifold.normal),
        };
        let mut points = [Point::default(); MAX_MANIFOLD_POINTS];
        let sources = manifold.active();
        for (point, source) in points.iter_mut().zip(sources) {
            *point = Point::new(source, &frame, setup);
        }
        Self {
            a: link.a,
            b: link.b,
            first: link.first,
            second: link.second,
            normal: frame.normal,
            tangents: frame.tangents,
            friction: setup.surface.friction,
            restitution: setup.surface.restitution,
            softness: setup.softness,
            count: sources.len(),
            points,
        }
    }

    /// The impulses of the points of the last step that are where these points are.
    fn warmed_from(&self, cached: &CachedManifold) -> Impulses {
        let mut taken = 0_u8;
        let mut impulses = Impulses::default();
        for (slot, point) in impulses.iter_mut().zip(&self.points).take(self.count) {
            *slot = point.warmed_from(cached, &self.tangents, &mut taken);
        }
        impulses
    }

    /// What is kept for the next step.
    fn cached(&self, impulses: &Impulses) -> CachedManifold {
        let mut kept = [CachedPoint::default(); MAX_MANIFOLD_POINTS];
        for ((slot, point), impulse) in kept
            .iter_mut()
            .zip(&self.points)
            .zip(impulses)
            .take(self.count)
        {
            *slot = CachedPoint {
                at: point.at,
                normal: impulse.normal,
                tangent: impulse.tangent_vector(&self.tangents),
            };
        }
        CachedManifold::new(
            self.first,
            self.second,
            kept.get(..self.count).unwrap_or(&[]),
        )
    }

    /// Applies the impulses of the last step at the start of a sub-step.
    fn warm_start(&self, impulses: &Impulses, ends: &mut Ends) {
        for (point, accumulated) in self.points.iter().zip(impulses).take(self.count) {
            let impulse =
                self.normal * accumulated.normal + accumulated.tangent_vector(&self.tangents);
            ends.push_apart(point, impulse);
        }
    }

    /// One sweep: friction first, then the normal, which is the more important one and so gets the
    /// last word.
    fn solve(&self, impulses: &mut Impulses, ends: &mut Ends, sweep: &Sweep) {
        self.solve_friction(impulses, ends);
        self.solve_normal(impulses, ends, sweep);
    }

    fn solve_friction(&self, impulses: &mut Impulses, ends: &mut Ends) {
        for (point, accumulated) in self.points.iter().zip(impulses).take(self.count) {
            self.friction_at(point, accumulated, ends);
        }
    }

    /// The friction solve of one point.
    #[inline]
    fn friction_at(&self, point: &Point, accumulated: &mut Impulse, ends: &mut Ends) {
        let relative = ends.relative_velocity(point);
        let before = accumulated.tangent;
        let wanted =
            [0, 1].map(|i| before[i] - point.tangent_mass[i] * relative.dot(self.tangents[i]));
        let after = within_cone(wanted, self.friction * accumulated.normal);
        accumulated.tangent = after;
        let applied = [0, 1].map(|i| after[i] - before[i]);
        let impulse = self.tangents[0] * applied[0] + self.tangents[1] * applied[1];
        ends.push_apart(point, impulse);
    }

    fn solve_normal(&self, impulses: &mut Impulses, ends: &mut Ends, sweep: &Sweep) {
        for (point, accumulated) in self.points.iter().zip(impulses).take(self.count) {
            self.normal_at(point, accumulated, ends, sweep);
        }
    }

    /// The normal solve of one point.
    #[inline]
    fn normal_at(&self, point: &Point, accumulated: &mut Impulse, ends: &mut Ends, sweep: &Sweep) {
        let moved = (ends.b.shift - ends.a.shift)
            + (ends.b.turn.rotate(point.anchor_b) - ends.a.turn.rotate(point.anchor_a));
        let separation = moved.dot(self.normal) + point.base_separation;
        let (bias, mass_scale, impulse_scale) = sweep.terms(separation, &self.softness);
        let approach = self.normal.dot(ends.relative_velocity(point));
        let delta = -point.normal_mass * mass_scale * (approach + bias)
            - impulse_scale * accumulated.normal;
        let applied = accumulated.accumulate(delta);
        ends.push_apart(point, self.normal * applied);
    }

    /// Bounce: after the last sub-step, a point that approached faster than the threshold and
    /// pushed at all gets the speed `restitution` times its approach speed away from the other body.
    fn restitute(&self, impulses: &mut Impulses, ends: &mut Ends, sweep: &Sweep) {
        if self.restitution == 0.0 {
            return;
        }
        let approach = -sweep.restitution_threshold;
        let bounces = |(point, accumulated): &(&Point, &mut Impulse)| {
            point.relative_velocity <= approach && accumulated.max_normal != 0.0
        };
        for (point, accumulated) in self
            .points
            .iter()
            .zip(impulses)
            .take(self.count)
            .filter(bounces)
        {
            let speed = self.normal.dot(ends.relative_velocity(point));
            let delta = -point.normal_mass * (speed + self.restitution * point.relative_velocity);
            let applied = accumulated.accumulate(delta);
            ends.push_apart(point, self.normal * applied);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base::math::Quat;
    use base::pool::Handle;

    const H: f32 = 1.0 / 240.0;

    fn handle(index: u32) -> BodyHandle<D3> {
        Handle::from_raw(index, 0)
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1.0e-5 * b.abs().max(1.0)
    }

    fn same(a: [f32; 2], b: [f32; 2]) -> bool {
        a.map(f32::to_bits) == b.map(f32::to_bits)
    }

    fn sweep(use_bias: bool) -> Sweep {
        Sweep {
            inv_h: 1.0 / H,
            use_bias,
            push_speed: 3.0,
            slop: 0.001,
            restitution_threshold: 1.0,
        }
    }

    /// A unit-mass ball (inverse inertia of a sphere of radius 0.5) and a body that cannot move.
    fn bodies() -> (SolverBody, SolverBody) {
        let mut ball = SolverBody::at_rest();
        ball.inv_mass = 1.0;
        ball.inv_inertia = base::math::Mat3::from_scale(Vec3::new(10.0, 10.0, 10.0));
        (SolverBody::at_rest(), ball)
    }

    /// One contact point at the origin, the normal up, from the fixed body to the ball.
    fn constraint(
        separation: f32,
        friction: f32,
        restitution: f32,
    ) -> (Constraint, SolverBody, SolverBody) {
        let (fixed, ball) = bodies();
        let manifold = Manifold3::new(
            Vec3::Y,
            &[ContactPoint3 {
                point: Vec3::ZERO,
                separation,
            }],
        );
        let setup = Setup {
            bodies: [&fixed, &ball],
            centres: [Vec3::new(0.0, -0.5, 0.0), Vec3::new(0.0, 0.5, 0.0)],
            surface: Surface {
                friction,
                restitution,
            },
            softness: Softness::new(30.0, 10.0, H),
        };
        let link = Link {
            first: handle(0),
            second: handle(1),
            a: 0,
            b: 1,
        };
        (Constraint::new(link, &manifold, &setup), fixed, ball)
    }

    fn solve_once(
        constraint: &mut Constraint,
        fixed: &mut SolverBody,
        ball: &mut SolverBody,
        use_bias: bool,
    ) {
        let mut ends = Ends { a: fixed, b: ball };
        constraint.solve(&mut ends, &sweep(use_bias));
    }

    fn lcg(state: &mut u64) -> f32 {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        f32::from(u16::try_from(*state >> 48).unwrap_or(0)) / 32_768.0 - 1.0
    }

    fn random_vec(state: &mut u64) -> Vec3 {
        Vec3::new(lcg(state), lcg(state), lcg(state))
    }

    /// A body that moves, with a velocity and a spin of its own (or one that cannot move).
    fn random_body(state: &mut u64, moves: bool) -> SolverBody {
        let mut body = SolverBody::at_rest();
        if moves {
            body.inv_mass = 1.0 + lcg(state).abs();
            body.inv_inertia = base::math::Mat3::from_scale(Vec3::new(8.0, 9.0, 11.0));
            body.velocity = random_vec(state);
            body.spin = random_vec(state);
        }
        body
    }

    /// A manifold of `points` points between body slots `a` and `b`, random in every number.
    fn random_constraint(
        state: &mut u64,
        points: usize,
        ends: (&SolverBody, &SolverBody),
        slots: (usize, usize),
    ) -> Constraint {
        let normal = random_vec(state).normalized().unwrap_or(Vec3::Y);
        let sources: Vec<ContactPoint3> = (0..points)
            .map(|_| ContactPoint3 {
                point: random_vec(state) * 0.5,
                separation: lcg(state) * 0.01,
            })
            .collect();
        let setup = Setup {
            bodies: [ends.0, ends.1],
            centres: [random_vec(state) * 0.5, random_vec(state) * 0.5],
            surface: Surface {
                friction: lcg(state).abs(),
                restitution: 0.0,
            },
            softness: Softness::new(30.0, 10.0, H),
        };
        let link = Link {
            first: handle(0),
            second: handle(1),
            a: slots.0,
            b: slots.1,
        };
        let mut made = Constraint::new(link, &Manifold3::new(normal, &sources), &setup);
        for slot in made.impulses.iter_mut().take(points) {
            slot.normal = lcg(state).abs();
            slot.tangent = [lcg(state) * 0.2, lcg(state) * 0.2];
        }
        made
    }

    fn same_body(x: &SolverBody, y: &SolverBody) -> bool {
        let bits = |v: Vec3| [v.x, v.y, v.z].map(f32::to_bits);
        bits(x.velocity) == bits(y.velocity) && bits(x.spin) == bits(y.spin)
    }

    fn same_impulses(x: &Impulses, y: &Impulses) -> bool {
        x.iter().zip(y).all(|(p, q)| {
            p.normal.to_bits() == q.normal.to_bits()
                && same(p.tangent, q.tangent)
                && p.max_normal.to_bits() == q.max_normal.to_bits()
        })
    }

    /// Two constraints on four bodies, solved one after the other or side by side.
    fn solved(
        constraints: [&Constraint; 2],
        bodies: &[SolverBody; 4],
        sweep: &Sweep,
        together: bool,
    ) -> (Constraint, Constraint, [SolverBody; 4]) {
        let (mut one, mut two) = (*constraints[0], *constraints[1]);
        let [mut a1, mut b1, mut a2, mut b2] = *bodies;
        if together {
            let (first, second) = (one.shape, two.shape);
            let mut left = Side {
                shape: &first,
                impulses: &mut one.impulses,
                ends: Ends {
                    a: &mut a1,
                    b: &mut b1,
                },
            };
            let mut right = Side {
                shape: &second,
                impulses: &mut two.impulses,
                ends: Ends {
                    a: &mut a2,
                    b: &mut b2,
                },
            };
            solve_pair(&mut left, &mut right, sweep);
        } else {
            one.solve(
                &mut Ends {
                    a: &mut a1,
                    b: &mut b1,
                },
                sweep,
            );
            two.solve(
                &mut Ends {
                    a: &mut a2,
                    b: &mut b2,
                },
                sweep,
            );
        }
        (one, two, [a1, b1, a2, b2])
    }

    #[test]
    fn two_constraints_solved_side_by_side_equal_the_two_solved_one_after_the_other_bit_for_bit() {
        let mut state = 0x5eed_u64;
        for trial in 0..400_usize {
            let (points_one, points_two) = (1 + trial % 4, 1 + (trial / 4) % 4);
            // Every fifth trial has bodies that cannot move on one end (a floor).
            let moves = trial % 5 != 0;
            let mut body = |n: usize| random_body(&mut state, moves || n.is_multiple_of(2));
            let bodies = [body(0), body(1), body(2), body(3)];
            let one = random_constraint(&mut state, points_one, (&bodies[0], &bodies[1]), (0, 1));
            let two = random_constraint(&mut state, points_two, (&bodies[2], &bodies[3]), (2, 3));
            let sweep = sweep(trial % 2 == 0);

            let alone = solved([&one, &two], &bodies, &sweep, false);
            let together = solved([&one, &two], &bodies, &sweep, true);

            assert!(
                same_impulses(&alone.0.impulses, &together.0.impulses),
                "{trial}"
            );
            assert!(
                same_impulses(&alone.1.impulses, &together.1.impulses),
                "{trial}"
            );
            let bodies_same = alone
                .2
                .iter()
                .zip(&together.2)
                .all(|(x, y)| same_body(x, y));
            assert!(bodies_same, "{trial}");
        }
    }

    #[test]
    fn the_tangents_are_unit_perpendicular_and_right_handed_for_every_kind_of_normal() {
        let normals = [
            Vec3::X,
            -Vec3::X,
            Vec3::Y,
            -Vec3::Y,
            Vec3::Z,
            -Vec3::Z,
            Vec3::new(1.0, 1.0, 1.0).normalized().unwrap_or(Vec3::X),
            Vec3::new(-0.6, 0.0, 0.8),
            Vec3::new(0.5, -0.5, std::f32::consts::FRAC_1_SQRT_2),
            Vec3::new(0.58, 0.8, 0.1).normalized().unwrap_or(Vec3::X),
        ];
        for normal in normals {
            let [first, second] = tangents_of(normal);
            assert!(
                near(first.length(), 1.0) && near(second.length(), 1.0),
                "{normal:?}"
            );
            assert!(first.dot(normal).abs() < 1.0e-5, "{normal:?}");
            assert!(second.dot(normal).abs() < 1.0e-5, "{normal:?}");
            assert!(first.dot(second).abs() < 1.0e-5, "{normal:?}");
            assert!(
                (first.cross(second) - normal).length() < 1.0e-5,
                "{normal:?}"
            );
        }
    }

    #[test]
    fn a_disc_limit_keeps_the_direction_and_a_bad_bound_gives_zero() {
        assert!(same(within_cone([3.0, 4.0], 10.0), [3.0, 4.0]));
        let shrunk = within_cone([3.0, 4.0], 2.5);
        assert!(near(shrunk[0], 1.5) && near(shrunk[1], 2.0), "{shrunk:?}");
        assert!(same(within_cone([3.0, 4.0], 0.0), [0.0, 0.0]));
        assert!(same(within_cone([3.0, 4.0], -1.0), [0.0, 0.0]));
        assert!(same(within_cone([3.0, 4.0], f32::NAN), [0.0, 0.0]));
    }

    #[test]
    fn a_soft_solve_pushes_an_overlap_out_with_the_spring_terms() {
        let (mut contact, mut fixed, mut ball) = constraint(-0.01, 0.0, 0.0);
        solve_once(&mut contact, &mut fixed, &mut ball, true);
        let soft = Softness::new(30.0, 10.0, H);
        // Unit mass, no approach: the speed given is mass_scale times the pull of the spring on
        // the overlap beyond the slop.
        let wanted = soft.mass_scale * soft.bias_rate * 0.009;
        assert!(
            near(ball.velocity.y, wanted),
            "{} against {wanted}",
            ball.velocity.y
        );
        assert_eq!(fixed.velocity, Vec3::ZERO);
    }

    #[test]
    fn the_relax_pass_pushes_a_resting_overlap_not_at_all() {
        let (mut contact, mut fixed, mut ball) = constraint(-0.01, 0.0, 0.0);
        solve_once(&mut contact, &mut fixed, &mut ball, false);
        assert!(ball.velocity.y.abs() < 1.0e-6);
    }

    #[test]
    fn the_overlap_is_read_from_the_relative_motion_of_both_bodies() {
        // Touching at the start; the first body moved up and the ball down by 1 cm each, so they
        // now overlap by 2 cm and the push has to answer that.
        let (mut contact, mut fixed, mut ball) = constraint(0.0, 0.0, 0.0);
        fixed.inv_mass = 1.0;
        fixed.shift = Vec3::new(0.0, 0.01, 0.0);
        ball.shift = Vec3::new(0.0, -0.01, 0.0);
        solve_once(&mut contact, &mut fixed, &mut ball, true);
        assert!(ball.velocity.y > 0.0, "{}", ball.velocity.y);
        assert!(fixed.velocity.y < 0.0, "{}", fixed.velocity.y);
    }

    #[test]
    fn the_turn_of_a_body_moves_its_anchor_when_the_overlap_is_read() {
        // Touching at a point 0.5 to the side of the ball's centre. The ball turns a quarter about
        // z, which carries that point to 0.5 below the centre, into the other body: an overlap.
        let (mut contact, mut fixed, mut ball) = constraint(0.0, 0.0, 0.0);
        contact.shape.points[0].anchor_a = Vec3::ZERO;
        contact.shape.points[0].anchor_b = Vec3::new(0.5, 0.0, 0.0);
        contact.shape.points[0].base_separation = 0.0;
        ball.turn =
            Quat::from_axis_angle(Vec3::Z, -std::f32::consts::FRAC_PI_2).unwrap_or(Quat::IDENTITY);
        solve_once(&mut contact, &mut fixed, &mut ball, true);
        assert!(ball.velocity.y > 0.1, "{}", ball.velocity.y);
    }

    #[test]
    fn friction_stops_a_sliding_ball_up_to_the_cone() {
        let (mut contact, mut fixed, mut ball) = constraint(0.0, 0.5, 0.0);
        // Pressed down so that the normal impulse is known and the cone has a size.
        contact.impulses[0].normal = 1.0;
        ball.velocity = Vec3::new(0.3, 0.0, 0.4);
        solve_once(&mut contact, &mut fixed, &mut ball, true);
        // The ball is a unit mass at the point, so a tangent impulse changes its velocity by that
        // much (and spins it, which the anchor along the normal does not feed back into the
        // sliding speed): the cone is 0.5 times the normal impulse, so the 0.5 m/s of sliding is
        // cut by at most 0.5, along the direction of the sliding.
        let sliding = Vec3::new(ball.velocity.x, 0.0, ball.velocity.z).length();
        assert!(sliding < 0.5 && sliding > 0.0, "{sliding}");
    }

    #[test]
    fn a_slow_approach_does_not_bounce_and_a_fast_one_does() {
        for (approach, bounces) in [(-0.5_f32, false), (-5.0, true)] {
            let (mut contact, mut fixed, mut ball) = constraint(0.0, 0.0, 1.0);
            contact.shape.points[0].relative_velocity = approach;
            contact.impulses[0].max_normal = 1.0;
            ball.velocity = Vec3::ZERO;
            let mut ends = Ends {
                a: &mut fixed,
                b: &mut ball,
            };
            contact.restitute(&mut ends, &sweep(false));
            assert_eq!(
                ball.velocity.y > 1.0,
                bounces,
                "approach {approach}: {:?}",
                ball.velocity
            );
        }
    }

    #[test]
    fn nothing_bounces_that_never_pushed() {
        let (mut contact, mut fixed, mut ball) = constraint(0.0, 0.0, 1.0);
        contact.shape.points[0].relative_velocity = -5.0;
        contact.impulses[0].max_normal = 0.0;
        let mut ends = Ends {
            a: &mut fixed,
            b: &mut ball,
        };
        contact.restitute(&mut ends, &sweep(false));
        assert_eq!(ball.velocity, Vec3::ZERO);
    }

    #[test]
    fn a_resting_contact_below_the_threshold_with_a_push_does_not_bounce() {
        // Approach slower than the threshold but with a push in the record: the second condition
        // alone must not trigger a bounce.
        let (mut contact, mut fixed, mut ball) = constraint(0.0, 0.0, 1.0);
        contact.shape.points[0].relative_velocity = -0.2;
        contact.impulses[0].max_normal = 2.0;
        let mut ends = Ends {
            a: &mut fixed,
            b: &mut ball,
        };
        contact.restitute(&mut ends, &sweep(false));
        assert!(ball.velocity.y.abs() < 1.0e-6, "{:?}", ball.velocity);
    }
}
