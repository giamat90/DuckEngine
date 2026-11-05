//! The 2D contact solver: a soft step in the manner of `Box2D` v3 **[K]** (owner choice O2 of
//! `wiki/architecture/physics-and-jobs-v0.md`). This is the scalar reference; the SIMD path of card
//! 4-15 has to equal it bit for bit, which is why every sweep is a plain loop over the constraints
//! in manifold-list order with `f32` operations in a fixed order.
//!
//! One step, with `n` sub-steps of length `h = dt / n`:
//!
//! 1. every manifold becomes a constraint (anchors, effective masses, approach speed, the
//!    impulses of the same features from the last step as the warm start);
//! 2. for each sub-step: integrate the velocities (gravity, forces, damping), apply the warm start,
//!    solve the constraints with the soft constraint (friction, then the normal), integrate the
//!    positions, then relax (solve again without the bias, so the push-out does not leave energy
//!    in the velocities);
//! 3. restitution, once, after the last sub-step;
//! 4. the impulses are kept for the next step.
//!
//! A body that no constraint touches skips all of this and takes one semi-implicit Euler step of
//! length `dt`, exactly as before the solver existed, so a world without contacts hashes as it did.

mod constraint;
mod warm;

use self::constraint::{
    Constraint, Ends, HERTZ_LIMIT_FRACTION, Link, STATIC_STIFFNESS_FACTOR, Setup, Softness,
    SolverBody, StepTerms, Surface, Sweep,
};
use self::warm::WarmCache;
use crate::core::{
    Body, BodyKind, ContactManifold, ContactSolver, Motion, PhysicsError, SolverConfig,
    StateHasher, StepFrame,
};
use crate::d2::D2;

/// The scalar contact solver of the 2D instance. All storage is allocated by [`Solver2::new`].
pub struct Solver2 {
    config: SolverConfig,
    /// `sub_steps` as a float, for the length of a sub-step.
    sub_steps: f32,
    /// The solver's view of the bodies, by slot index; valid where `touched` is set.
    bodies: Vec<SolverBody>,
    touched: Vec<bool>,
    /// Slot indices of the touched dynamic bodies.
    active: Vec<usize>,
    constraints: Vec<Constraint>,
    /// The most constraints one step can hold: the manifold capacity.
    max_constraints: usize,
    /// The impulses of the last committed step.
    warm: WarmCache,
    /// The impulses of the step being solved.
    next: WarmCache,
}

/// The two ends of a constraint, or `None` when a body is missing (cannot happen for manifolds the
/// world made, which are rebuilt after every change of the bodies).
fn ends_of<'a>(
    bodies: &'a [Option<Body<D2>>],
    entry: &ContactManifold<D2>,
) -> Option<(usize, usize, &'a Body<D2>, &'a Body<D2>)> {
    let (a, b) = (entry.first.index() as usize, entry.second.index() as usize);
    let first = bodies.get(a)?.as_ref()?;
    let second = bodies.get(b)?.as_ref()?;
    Some((a, b, first, second))
}

/// What `prepare` needs besides the solver's own storage and the manifold at hand.
struct Prepare<'a> {
    bodies: &'a [Option<Body<D2>>],
    terms: StepTerms,
    /// Softness of a contact between two dynamic bodies.
    soft: Softness,
    /// Softness of a contact with a static body.
    stiff: Softness,
}

/// Applies `visit` to the solver body at `index`, if there is one.
fn visit_body(bodies: &mut [SolverBody], index: usize, visit: &impl Fn(&mut SolverBody)) {
    if let Some(body) = bodies.get_mut(index) {
        visit(body);
    }
}

/// Runs `visit` on `constraint` with copies of its two bodies, which are stored back afterwards.
fn visit_constraint(
    bodies: &mut [SolverBody],
    constraint: &mut Constraint,
    visit: &impl Fn(&mut Constraint, &mut Ends),
) {
    let (Some(&a), Some(&b)) = (bodies.get(constraint.a), bodies.get(constraint.b)) else {
        return;
    };
    let (mut a, mut b) = (a, b);
    visit(
        constraint,
        &mut Ends {
            a: &mut a,
            b: &mut b,
        },
    );
    if let Some(slot) = bodies.get_mut(constraint.a) {
        *slot = a;
    }
    if let Some(slot) = bodies.get_mut(constraint.b) {
        *slot = b;
    }
}

impl ContactSolver<D2> for Solver2 {
    fn new(
        config: SolverConfig,
        max_bodies: u32,
        max_manifolds: u32,
    ) -> Result<Self, PhysicsError> {
        config.validate()?;
        let bodies = max_bodies as usize;
        let manifolds = max_manifolds as usize;
        let count =
            u8::try_from(config.sub_steps).map_err(|_| PhysicsError::InvalidSolverConfig)?;
        Ok(Self {
            config,
            sub_steps: f32::from(count),
            bodies: vec![SolverBody::AT_REST; bodies],
            touched: vec![false; bodies],
            active: Vec::with_capacity(bodies),
            constraints: Vec::with_capacity(manifolds),
            max_constraints: manifolds,
            warm: WarmCache::new(manifolds),
            next: WarmCache::new(manifolds),
        })
    }

    fn solve(
        &mut self,
        bodies: &mut [Option<Body<D2>>],
        contacts: &[ContactManifold<D2>],
        frame: StepFrame<D2>,
    ) {
        let h = frame.dt / self.sub_steps;
        let terms = StepTerms {
            gravity: frame.gravity,
            h,
        };
        self.prepare(bodies, contacts, terms);
        self.run_sub_steps(h);
        self.finish(bodies, &frame);
    }

    fn is_finite(&self) -> bool {
        self.next.is_finite()
    }

    fn commit(&mut self) {
        std::mem::swap(&mut self.warm, &mut self.next);
    }

    fn write_bits(&self, hasher: &mut StateHasher) {
        self.warm.write_bits(hasher);
    }
}

impl Solver2 {
    /// The contact softness for dynamic and static partners at sub-step length `h`.
    fn softness(&self, h: f32) -> (Softness, Softness) {
        let hertz = self.config.contact_hertz.min(HERTZ_LIMIT_FRACTION / h);
        let ratio = self.config.contact_damping_ratio;
        (
            Softness::new(hertz, ratio, h),
            Softness::new(STATIC_STIFFNESS_FACTOR * hertz, ratio, h),
        )
    }

    /// Sets up the solver body of slot `index` the first time a constraint touches it.
    fn touch(&mut self, index: usize, body: &Body<D2>, terms: StepTerms) {
        let Some(seen) = self.touched.get_mut(index) else {
            return;
        };
        if std::mem::replace(seen, true) {
            return;
        }
        if let Some(slot) = self.bodies.get_mut(index) {
            *slot = SolverBody::new(body, terms);
        }
        if body.kind() == BodyKind::Dynamic {
            self.active.push(index);
        }
    }

    /// Makes the constraints of `contacts` in their order, with the warm start.
    fn prepare(
        &mut self,
        bodies: &[Option<Body<D2>>],
        contacts: &[ContactManifold<D2>],
        terms: StepTerms,
    ) {
        self.touched.fill(false);
        self.active.clear();
        self.constraints.clear();
        let (soft, stiff) = self.softness(terms.h);
        let context = Prepare {
            bodies,
            terms,
            soft,
            stiff,
        };
        let mut cursor = 0;
        for entry in contacts {
            self.add_constraint(&context, entry, &mut cursor);
        }
    }

    /// Adds the constraint of one manifold; a manifold of a missing body, or one that does not fit
    /// (it always does: the capacity is the manifold capacity), is left out.
    fn add_constraint(
        &mut self,
        context: &Prepare,
        entry: &ContactManifold<D2>,
        cursor: &mut usize,
    ) {
        if self.constraints.len() >= self.max_constraints {
            return;
        }
        let Some((a, b, first, second)) = ends_of(context.bodies, entry) else {
            return;
        };
        self.touch(a, first, context.terms);
        self.touch(b, second, context.terms);
        let (Some(body_a), Some(body_b)) = (self.bodies.get(a), self.bodies.get(b)) else {
            return;
        };
        let setup = Setup {
            bodies: [body_a, body_b],
            surface: Surface {
                friction: (first.friction() * second.friction()).sqrt(),
                restitution: first.restitution().max(second.restitution()),
            },
            softness: if body_a.is_static() || body_b.is_static() {
                context.stiff
            } else {
                context.soft
            },
        };
        let link = Link {
            first: entry.first,
            second: entry.second,
            a,
            b,
        };
        let mut constraint = Constraint::new(link, &entry.manifold, &setup);
        if let Some(cached) = self.warm.find(cursor, entry.first, entry.second) {
            constraint.warm_from(cached);
        }
        self.constraints.push(constraint);
    }

    /// The sub-step loop, then restitution.
    fn run_sub_steps(&mut self, h: f32) {
        let solve = self.sweep(h, true);
        let relax = self.sweep(h, false);
        for _ in 0..self.config.sub_steps {
            self.sub_step(h, [&solve, &relax]);
        }
        self.sweep_constraints(|constraint, ends| constraint.restitute(ends, &relax));
    }

    /// One sub-step: velocities, warm start, soft solve, positions, relax.
    fn sub_step(&mut self, h: f32, [solve, relax]: [&Sweep; 2]) {
        self.each_active(|body| body.integrate_velocity(h));
        self.sweep_constraints(|constraint, ends| constraint.warm_start(ends));
        self.sweep_constraints(|constraint, ends| constraint.solve(ends, solve));
        self.each_active(|body| body.integrate_position(h));
        for _ in 0..self.config.relax_iterations {
            self.sweep_constraints(|constraint, ends| constraint.solve(ends, relax));
        }
    }

    fn sweep(&self, h: f32, use_bias: bool) -> Sweep {
        Sweep {
            inv_h: 1.0 / h,
            use_bias,
            push_speed: self.config.contact_push_speed,
            slop: self.config.linear_slop,
            restitution_threshold: self.config.restitution_threshold,
        }
    }

    /// Runs `visit` over the touched dynamic bodies, in the order they were first touched.
    fn each_active(&mut self, visit: impl Fn(&mut SolverBody)) {
        for &index in &self.active {
            visit_body(&mut self.bodies, index, &visit);
        }
    }

    /// Runs `visit` over the constraints in order.
    fn sweep_constraints(&mut self, visit: impl Fn(&mut Constraint, &mut Ends)) {
        for constraint in &mut self.constraints {
            visit_constraint(&mut self.bodies, constraint, &visit);
        }
    }

    /// Keeps the impulses and writes the new motion into the bodies.
    fn finish(&mut self, bodies: &mut [Option<Body<D2>>], frame: &StepFrame<D2>) {
        self.next.clear();
        for constraint in &self.constraints {
            self.next.push(constraint.cached());
        }
        for (index, slot) in bodies.iter_mut().enumerate() {
            self.write_back(index, slot, frame);
        }
    }

    /// Writes the solved motion into the body of slot `index` if a constraint touched it; any other
    /// dynamic body takes the plain Euler step.
    fn write_back(&self, index: usize, slot: &mut Option<Body<D2>>, frame: &StepFrame<D2>) {
        let Some(body) = slot else {
            return;
        };
        if body.kind() == BodyKind::Static {
            return;
        }
        match (self.touched.get(index), self.bodies.get(index)) {
            (Some(true), Some(solved)) => body.set_motion(&Motion {
                position: body.position() + solved.shift,
                rotation: body.rotation().turned_by(solved.turn),
                linear_velocity: solved.velocity,
                angular_velocity: solved.spin,
            }),
            _ => body.integrate(frame.gravity, frame.dt),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::warm::{CachedManifold, CachedPoint, Impulse};
    use super::*;
    use base::pool::Handle;

    #[test]
    fn a_solver_whose_impulses_are_not_numbers_is_not_finite() {
        let Ok(mut solver) = Solver2::new(SolverConfig::DEFAULT, 4, 4) else {
            return;
        };
        assert!(solver.is_finite());
        let point = CachedPoint {
            impulse: Impulse {
                normal: f32::NAN,
                tangent: 0.0,
            },
            ..CachedPoint::default()
        };
        solver.next.push(CachedManifold::new(
            Handle::from_raw(0, 0),
            Handle::from_raw(1, 0),
            &[point],
        ));
        assert!(!solver.is_finite());
    }
}
