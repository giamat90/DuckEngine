//! The 3D contact solver: a soft step in the manner of the 2D one (`d2::solver`), on the same
//! `ContactSolver` seam. Scalar: the constraints are swept colour by colour (card 8-19,
//! `core::colouring`), and with a scheduler in the execution the chunks of a colour run in parallel.
//!
//! One step, with `n` sub-steps of length `h = dt / n`:
//!
//! 1. every manifold becomes a constraint (anchors, effective masses along the normal and two
//!    tangents, approach speed, the impulses of the points that are where these points are, from
//!    the last step, as the warm start);
//! 2. for each sub-step: integrate the velocities (gravity, forces, damping), apply the warm start,
//!    solve the constraints with the soft constraint (friction on two tangents inside a circular
//!    cone, then the normal), integrate the positions, then relax (solve again without the bias);
//! 3. restitution, once, after the last sub-step;
//! 4. the impulses are kept for the next step.
//!
//! A constraint with no awake dynamic body (all its bodies sleep or are static) is not solved; its
//! warm start is kept as it is. A body that no constraint touches takes one semi-implicit Euler
//! step of length `dt`, exactly as before the solver existed. Joints (card 8-7) are
//! solved first in every pass, serially, see `joint`. The integrator has no gyroscopic term
//! (card 8-3).
//!
//! The constraints are swept colour by colour: no two constraints of a colour share a moving body,
//! so inside a colour the order does not matter (the tests shuffle it and compare state hashes) and
//! a colour can be solved in parallel (`stage`, the same as the 2D solver, without lanes). The
//! colours are visited in order, the constraints of a colour in manifold-list order, the overflow
//! set (constraints that found no colour) last, serially. The serial colour order is the
//! reference: every execution gives the same bits.

mod body;
mod constraint;
mod joint;
mod stage;
mod warm;

#[cfg(test)]
mod colouring_tests;

use self::body::{SolverBody, StepTerms};
use self::constraint::{Constraint, Ends, Link, Pass, Setup, Surface};
use self::joint::{JointConstraint, JointPass, JointSet, JointSetup};
use self::stage::{ChunkOut, ColourJob, make_outputs, scatter};
use self::warm::WarmCache;
use crate::core::cells::index_u32;
use crate::core::soft::{
    HERTZ_LIMIT_FRACTION, JointSweep, STATIC_STIFFNESS_FACTOR, Softness, Sweep, rigid_softness,
};
use crate::core::{
    Body, BodyKind, Colouring, ConstraintEnds, ContactManifold, ContactSolver, JointEntry, Motion,
    PhysicsError, SolverConfig, SolverExecution, SolverStats, StateHasher, StepFrame,
};
use crate::d3::{D3, Joint3};
use base::fpenv::FpGuard;
use std::sync::Mutex;

/// The scalar contact solver of the 3D instance. All storage is allocated by [`Solver3::new`].
pub struct Solver3 {
    config: SolverConfig,
    /// `sub_steps` as a float, for the length of a sub-step.
    sub_steps: f32,
    /// The solver's view of the bodies, by slot index; valid where `touched` is set.
    bodies: Vec<SolverBody>,
    touched: Vec<bool>,
    /// Slot indices of the touched dynamic bodies.
    active: Vec<usize>,
    constraints: Vec<Constraint>,
    /// The buffers of the staged path, one per chunk of the largest possible colour; empty while
    /// the execution is the plain serial reference.
    outputs: Vec<Mutex<ChunkOut>>,
    /// The chunk size `outputs` was made for.
    outputs_chunk: usize,
    /// How the sweeps execute (serial reference, scheduler).
    execution: SolverExecution,
    /// The colours of the constraints; the sweeps follow its order.
    colouring: Colouring,
    /// The most constraints one step can hold: the manifold capacity.
    max_constraints: usize,
    /// The impulses of the last committed step.
    warm: WarmCache,
    /// The impulses of the step being solved.
    next: WarmCache,
    /// The joints of the step. They are solved serially, before the contacts, in every pass.
    joints: JointSet,
    /// Reorders the constraints inside each colour after the colouring (tests only).
    #[cfg(test)]
    reorder: Option<Reorder>,
}

/// How a test reorders the constraints inside a colour.
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(crate) enum Reorder {
    /// A Fisher-Yates shuffle from the seed.
    Shuffle(u64),
    /// The reverse order.
    Reverse,
}

/// The two ends of a constraint, or `None` when a body is missing (cannot happen for manifolds the
/// world made, which are rebuilt after every change of the bodies).
fn ends_of<'a>(
    bodies: &'a [Option<Body<D3>>],
    entry: &ContactManifold<D3>,
) -> Option<(usize, usize, &'a Body<D3>, &'a Body<D3>)> {
    let (a, b) = (entry.first.index() as usize, entry.second.index() as usize);
    let first = bodies.get(a)?.as_ref()?;
    let second = bodies.get(b)?.as_ref()?;
    Some((a, b, first, second))
}

/// True for a body the step moves: dynamic and not asleep.
fn is_awake(body: &Body<D3>) -> bool {
    body.kind() == BodyKind::Dynamic && !body.is_asleep()
}

/// What `prepare` needs besides the solver's own storage and the manifold at hand.
struct Prepare<'a> {
    bodies: &'a [Option<Body<D3>>],
    terms: StepTerms,
    /// Softness of a contact between two dynamic bodies.
    soft: Softness,
    /// Softness of a contact with a static body.
    stiff: Softness,
}

/// Applies `step` to the solver body at `index`, if there is one.
fn visit_body(bodies: &mut [SolverBody], index: usize, step: &impl Fn(&mut SolverBody)) {
    if let Some(body) = bodies.get_mut(index) {
        step(body);
    }
}

/// Runs `pass` on the constraint `index` of the step, if there is one.
fn visit_member(constraints: &mut [Constraint], bodies: &mut [SolverBody], index: u32, pass: Pass) {
    if let Some(constraint) = constraints.get_mut(index as usize) {
        visit(bodies, constraint, pass);
    }
}

/// Runs `pass` on the constraints `members` one after the other.
fn visit_members(
    constraints: &mut [Constraint],
    bodies: &mut [SolverBody],
    members: &[u32],
    pass: Pass,
) {
    for &index in members {
        visit_member(constraints, bodies, index, pass);
    }
}

/// Runs `pass` on `constraint` with copies of its two bodies, which are stored back afterwards.
fn visit(bodies: &mut [SolverBody], constraint: &mut Constraint, pass: Pass) {
    let (Some(&first), Some(&second)) = (
        bodies.get(constraint.shape.a),
        bodies.get(constraint.shape.b),
    ) else {
        return;
    };
    let (mut a, mut b) = (first, second);
    pass.apply(
        constraint,
        &mut Ends {
            a: &mut a,
            b: &mut b,
        },
    );
    store(bodies, constraint.shape.a, a);
    store(bodies, constraint.shape.b, b);
}

/// Stores `body` at `index` unless it is a static body, which did not change.
fn store(bodies: &mut [SolverBody], index: usize, body: SolverBody) {
    if body.is_static() {
        return;
    }
    if let Some(slot) = bodies.get_mut(index) {
        *slot = body;
    }
}

impl ContactSolver<D3> for Solver3 {
    fn new(
        config: SolverConfig,
        max_bodies: u32,
        max_manifolds: u32,
        max_joints: u32,
    ) -> Result<Self, PhysicsError> {
        config.validate()?;
        let bodies = max_bodies as usize;
        let manifolds = max_manifolds as usize;
        let count =
            u8::try_from(config.sub_steps).map_err(|_| PhysicsError::InvalidSolverConfig)?;
        Ok(Self {
            config,
            sub_steps: f32::from(count),
            bodies: vec![SolverBody::at_rest(); bodies],
            touched: vec![false; bodies],
            active: Vec::with_capacity(bodies),
            constraints: Vec::with_capacity(manifolds),
            outputs: Vec::new(),
            outputs_chunk: 0,
            execution: SolverExecution::SERIAL,
            colouring: Colouring::new(bodies, manifolds),
            max_constraints: manifolds,
            warm: WarmCache::new(manifolds),
            next: WarmCache::new(manifolds),
            joints: JointSet::new(max_joints as usize),
            #[cfg(test)]
            reorder: None,
        })
    }

    fn solve(
        &mut self,
        bodies: &mut [Option<Body<D3>>],
        contacts: &[ContactManifold<D3>],
        joints: &[JointEntry<D3>],
        frame: StepFrame<D3>,
    ) {
        // One deterministic floating-point state for the whole solve; restores what it found.
        let _guard = FpGuard::deterministic();
        let h = frame.dt / self.sub_steps;
        let terms = StepTerms {
            gravity: frame.gravity,
            h,
        };
        self.prepare(bodies, contacts, joints, terms);
        self.colouring.assign();
        #[cfg(test)]
        self.apply_reorder();
        self.run_sub_steps(h);
        self.finish(bodies, &frame);
    }

    fn set_execution(&mut self, execution: SolverExecution) {
        self.execution = execution;
        self.size_outputs();
    }

    fn is_finite(&self) -> bool {
        self.next.is_finite() && self.joints.is_finite()
    }

    fn stats(&self) -> SolverStats {
        let (joints_solved, joints_frozen) = self.joints.counts();
        SolverStats {
            solved: self.colouring.order().len(),
            frozen: self.constraints.len() - self.colouring.order().len(),
            colours: self.colouring.colour_count(),
            overflow: self.colouring.overflow().len(),
            joints_solved,
            joints_frozen,
        }
    }

    fn commit(&mut self) {
        std::mem::swap(&mut self.warm, &mut self.next);
        self.joints.commit();
    }

    fn write_bits(&self, hasher: &mut StateHasher) {
        self.warm.write_bits(hasher);
        self.joints.write_bits(hasher);
    }
}

impl Solver3 {
    /// Applies the reordering a test asked for, if any.
    #[cfg(test)]
    fn apply_reorder(&mut self) {
        match self.reorder {
            Some(Reorder::Shuffle(seed)) => self.colouring.shuffle_within_colours(seed),
            Some(Reorder::Reverse) => self.colouring.reverse_within_colours(),
            None => {}
        }
    }

    /// Asks the next solves to reorder the constraints inside each colour (tests only).
    #[cfg(test)]
    pub(crate) fn set_reorder(&mut self, reorder: Option<Reorder>) {
        self.reorder = reorder;
    }

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
    fn touch(&mut self, index: usize, body: &Body<D3>, terms: StepTerms) {
        let Some(seen) = self.touched.get_mut(index) else {
            return;
        };
        if std::mem::replace(seen, true) {
            return;
        }
        if let Some(slot) = self.bodies.get_mut(index) {
            *slot = SolverBody::new(body, terms);
        }
        if is_awake(body) {
            self.active.push(index);
        }
    }

    /// Makes the constraints of `contacts` in their order, with the warm start.
    fn prepare(
        &mut self,
        bodies: &[Option<Body<D3>>],
        contacts: &[ContactManifold<D3>],
        joints: &[JointEntry<D3>],
        terms: StepTerms,
    ) {
        self.touched.fill(false);
        self.active.clear();
        self.constraints.clear();
        self.colouring.clear();
        self.joints.clear();
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
        for entry in joints {
            self.add_joint(&context, entry);
        }
    }

    /// Adds the joint of `entry`, which is solved when one of its bodies is awake. Joints come after
    /// the contacts, so the bodies a world without joints touches are touched in the same order.
    fn add_joint(&mut self, context: &Prepare, entry: &JointEntry<D3>) {
        let (a, b) = (entry.first.index() as usize, entry.second.index() as usize);
        let (Some(Some(first)), Some(Some(second))) =
            (context.bodies.get(a), context.bodies.get(b))
        else {
            return;
        };
        self.touch(a, first, context.terms);
        self.touch(b, second, context.terms);
        let setup = JointSetup {
            handle: entry.handle,
            slots: (a, b),
            bodies: [first, second],
        };
        let mut joint = match entry.shape {
            Joint3::Ball(shape) => JointConstraint::ball(&setup, &shape),
            Joint3::Hinge(shape) => JointConstraint::hinge(&setup, &shape),
            Joint3::Distance(shape) => JointConstraint::rod(&setup, &shape),
        };
        joint.solved = is_awake(first) || is_awake(second);
        self.joints.add(joint);
    }

    /// Adds the constraint of one manifold; a manifold of a missing body, or one that does not fit
    /// (it always does: the capacity is the manifold capacity), is left out.
    fn add_constraint(
        &mut self,
        context: &Prepare,
        entry: &ContactManifold<D3>,
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
            centres: [first.position(), second.position()],
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
        self.colouring.push(ConstraintEnds {
            a: is_awake(first).then(|| index_u32(a)),
            b: is_awake(second).then(|| index_u32(b)),
        });
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
        let joint_solve = self.joint_sweep(h, true);
        let joint_relax = self.joint_sweep(h, false);
        for _ in 0..self.config.sub_steps {
            self.sub_step(h, [&solve, &relax], [&joint_solve, &joint_relax]);
        }
        self.sweep_constraints(Pass::Restitute(&relax));
    }

    /// One sub-step: velocities, warm start, soft solve, positions, relax.
    fn sub_step(
        &mut self,
        h: f32,
        [solve, relax]: [&Sweep; 2],
        [joint_solve, joint_relax]: [&JointSweep; 2],
    ) {
        self.each_active(|body| body.integrate_velocity(h));
        self.joints.sweep(&mut self.bodies, JointPass::WarmStart);
        self.sweep_constraints(Pass::WarmStart);
        self.joints
            .sweep(&mut self.bodies, JointPass::Solve(joint_solve));
        self.sweep_constraints(Pass::Solve(solve));
        self.each_active(|body| body.integrate_position(h));
        for _ in 0..self.config.relax_iterations {
            self.joints
                .sweep(&mut self.bodies, JointPass::Solve(joint_relax));
            self.sweep_constraints(Pass::Solve(relax));
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

    fn joint_sweep(&self, h: f32, use_bias: bool) -> JointSweep {
        JointSweep {
            h,
            inv_h: 1.0 / h,
            use_bias,
            rigid: rigid_softness(
                self.config.joint_hertz,
                self.config.joint_damping_ratio,
                HERTZ_LIMIT_FRACTION,
                h,
            ),
        }
    }

    /// Runs `visit` over the touched dynamic bodies, in the order they were first touched.
    fn each_active(&mut self, step: impl Fn(&mut SolverBody)) {
        for &index in &self.active {
            visit_body(&mut self.bodies, index, &step);
        }
    }

    /// Runs `pass` over the constraints that are solved, colour by colour (see the module
    /// comment). The reference visits them one by one; with a scheduler in the execution each
    /// colour is solved as chunks (`stage`) and the overflow set follows serially.
    fn sweep_constraints(&mut self, pass: Pass) {
        let total = self.colouring.order().len();
        if !self.execution.is_staged() {
            return self.visit_order(0..total, pass);
        }
        for colour in 0..self.colouring.colour_count() {
            self.stage_colour(colour, pass);
        }
        self.visit_order(total - self.colouring.overflow().len()..total, pass);
    }

    /// The reference loop over the positions `range` of the solve order: one constraint after the
    /// other on the calling thread.
    fn visit_order(&mut self, range: std::ops::Range<usize>, pass: Pass) {
        let part = self.colouring.order().get(range).unwrap_or(&[]);
        visit_members(&mut self.constraints, &mut self.bodies, part, pass);
    }

    /// Makes the buffers of the staged path for the current execution (once per change of the
    /// chunk size; setting the same execution again allocates nothing).
    fn size_outputs(&mut self) {
        if !self.execution.is_staged() {
            self.outputs = Vec::new();
            self.outputs_chunk = 0;
        } else if self.outputs_chunk != self.execution.chunk_size() {
            self.outputs_chunk = self.execution.chunk_size();
            self.outputs = make_outputs(self.max_constraints, self.outputs_chunk);
        }
    }

    /// Solves colour `colour` as fixed chunks, then stores the results back in canonical order.
    fn stage_colour(&mut self, colour: usize, pass: Pass) {
        let members = self.colouring.members(colour);
        let job = ColourJob {
            constraints: &self.constraints,
            bodies: &self.bodies,
            members,
            pass,
            chunk: self.execution.chunk_size(),
            outputs: &self.outputs,
        };
        if !job.has_room() {
            // Not reachable for a solver that sized its buffers, kept so that no chunk is lost.
            return visit_members(&mut self.constraints, &mut self.bodies, members, pass);
        }
        let chunks = job.chunk_count();
        job.run(self.execution.scheduler.as_deref());
        scatter(
            &mut self.outputs,
            chunks,
            &mut self.constraints,
            &mut self.bodies,
        );
    }

    /// Keeps the impulses and writes the new motion into the bodies.
    fn finish(&mut self, bodies: &mut [Option<Body<D3>>], frame: &StepFrame<D3>) {
        self.next.clear();
        for constraint in &self.constraints {
            self.next.push(constraint.cached());
        }
        self.joints.finish();
        for (index, slot) in bodies.iter_mut().enumerate() {
            self.write_back(index, slot, frame);
        }
    }

    /// Writes the solved motion into the body of slot `index` if a constraint touched it; any other
    /// dynamic body takes the plain Euler step.
    fn write_back(&self, index: usize, slot: &mut Option<Body<D3>>, frame: &StepFrame<D3>) {
        let Some(body) = slot else {
            return;
        };
        if body.kind() == BodyKind::Static || body.is_asleep() {
            return;
        }
        match (self.touched.get(index), self.bodies.get(index)) {
            (Some(true), Some(solved)) => {
                let turned = solved.turn * body.rotation();
                body.set_motion(&Motion {
                    position: body.position() + solved.shift,
                    rotation: turned.normalized().unwrap_or_else(|| body.rotation()),
                    linear_velocity: solved.velocity,
                    angular_velocity: solved.spin,
                });
            }
            _ => body.integrate(frame.gravity, frame.dt),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::soft::Softness;
    use crate::d3::{BodyDesc, Shape3, SleepConfig, World, WorldConfig};
    use base::math::Vec3;
    use base::pool::Handle;

    const H: f32 = 1.0 / 240.0;

    fn solver() -> Solver3 {
        match Solver3::new(SolverConfig::DEFAULT, 4, 4, 0) {
            Ok(solver) => solver,
            Err(error) => panic!("a default solver: {error:?}"),
        }
    }

    fn kept(normal: f32) -> warm::CachedManifold {
        warm::CachedManifold::new(
            Handle::from_raw(0, 0),
            Handle::from_raw(1, 0),
            &[warm::CachedPoint {
                at: Vec3::ZERO,
                normal,
                tangent: Vec3::ZERO,
            }],
        )
    }

    fn hash_of(solver: &Solver3) -> u64 {
        let mut hasher = StateHasher::new();
        solver.write_bits(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn a_solver_whose_impulses_are_not_numbers_is_not_finite() {
        let mut solver = solver();
        assert!(solver.is_finite());
        solver.next.push(kept(f32::NAN));
        assert!(!solver.is_finite());
    }

    #[test]
    fn what_is_committed_is_what_the_hash_and_the_next_step_see() {
        let mut solver = solver();
        let empty = hash_of(&solver);
        solver.next.push(kept(2.0));
        // Not committed yet: the hash is of the last committed step.
        assert_eq!(hash_of(&solver), empty);
        solver.commit();
        assert_ne!(hash_of(&solver), empty);
    }

    #[test]
    fn the_softness_of_a_contact_is_the_spring_and_twice_the_spring_against_a_static_body() {
        let solver = solver();
        let (soft, stiff) = solver.softness(H);
        assert_eq!(soft, Softness::new(30.0, 10.0, H));
        assert_eq!(stiff, Softness::new(60.0, 10.0, H));
        // A spring above a quarter of the sub-step rate is capped there.
        let mut fast = SolverConfig::DEFAULT;
        fast.contact_hertz = 1000.0;
        let capped = match Solver3::new(fast, 1, 1, 0) {
            Ok(solver) => solver,
            Err(error) => panic!("{error:?}"),
        };
        let (soft, stiff) = capped.softness(H);
        assert_eq!(soft, Softness::new(60.0, 10.0, H));
        assert_eq!(stiff, Softness::new(120.0, 10.0, H));
    }

    #[test]
    fn a_sweep_asks_for_the_sub_step_rate_and_the_configured_limits() {
        let solver = solver();
        let sweep = solver.sweep(H, true);
        assert!((sweep.inv_h - 240.0).abs() < 1.0e-3);
        assert!(sweep.use_bias && !solver.sweep(H, false).use_bias);
        assert!((sweep.push_speed - SolverConfig::DEFAULT.contact_push_speed).abs() < 1.0e-9);
        assert!((sweep.slop - SolverConfig::DEFAULT.linear_slop).abs() < 1.0e-9);
    }

    fn world() -> Result<World, PhysicsError> {
        World::new(WorldConfig {
            max_bodies: 4,
            gravity: Vec3::new(0.0, -9.8, 0.0),
            max_pairs: 16,
            broadphase_margin: crate::d3::DEFAULT_BROADPHASE_MARGIN,
            max_manifolds: 16,
            max_joints: 0,
            speculative_margin: crate::d3::DEFAULT_SPECULATIVE_MARGIN,
            solver: SolverConfig::DEFAULT,
            sleep: SleepConfig::DISABLED,
        })
    }

    fn crate_on_a_floor(w: &mut World) -> Result<(), PhysicsError> {
        let half = Shape3::Box {
            half_extents: Vec3::new(0.5, 0.5, 0.5),
        };
        w.create_body(&BodyDesc::new(
            BodyKind::Static,
            Vec3::new(0.0, -0.5, 0.0),
            half,
        ))?;
        w.create_body(&BodyDesc::new(
            BodyKind::Dynamic,
            Vec3::new(0.0, 0.499, 0.0),
            half,
        ))?;
        Ok(())
    }

    #[test]
    fn a_contact_with_one_awake_body_and_one_static_body_is_solved() -> Result<(), PhysicsError> {
        let mut awake = world()?;
        crate_on_a_floor(&mut awake)?;
        awake.step(1.0 / 60.0)?;
        let stats = awake.solver_stats();
        assert_eq!((stats.solved, stats.frozen), (1, 0), "{stats:?}");
        Ok(())
    }

    #[test]
    fn a_contact_against_a_static_body_is_made_stiffer_than_one_between_two_dynamic_bodies()
    -> Result<(), PhysicsError> {
        let mut with_floor = world()?;
        crate_on_a_floor(&mut with_floor)?;
        let mut solver = solver();
        let bodies: Vec<Option<Body<D3>>> = with_floor.iter().map(|(_, b)| Some(*b)).collect();
        let mut loose = world()?;
        let half = Shape3::Box {
            half_extents: Vec3::new(0.5, 0.5, 0.5),
        };
        loose.create_body(&BodyDesc::new(
            BodyKind::Dynamic,
            Vec3::new(0.0, -0.5, 0.0),
            half,
        ))?;
        loose.create_body(&BodyDesc::new(
            BodyKind::Dynamic,
            Vec3::new(0.0, 0.499, 0.0),
            half,
        ))?;
        loose.find_pairs();
        loose.collide();
        with_floor.find_pairs();
        with_floor.collide();
        let terms = StepTerms {
            gravity: Vec3::ZERO,
            h: H,
        };
        let (soft, stiff) = solver.softness(H);
        solver.prepare(&bodies, with_floor.manifolds().as_slice(), &[], terms);
        assert_eq!(solver.constraints[0].softness(), stiff);
        let loose_bodies: Vec<Option<Body<D3>>> = loose.iter().map(|(_, b)| Some(*b)).collect();
        solver.prepare(&loose_bodies, loose.manifolds().as_slice(), &[], terms);
        assert_eq!(solver.constraints[0].softness(), soft);
        Ok(())
    }
}
