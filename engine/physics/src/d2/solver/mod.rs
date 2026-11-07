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
//! The constraints are not swept in manifold-list order but colour by colour (card 4-14, `core::colouring`):
//! no two constraints of a colour share a dynamic body, so inside a colour the order does not matter
//! (the tests shuffle it and compare state hashes) and card 4-15 can solve a colour in parallel. The
//! colours are visited in order, the constraints of a colour in manifold-list order, the overflow set
//! (constraints that found no colour) last, serially. A constraint with no awake dynamic body (all
//! its bodies sleep or are static) is not solved; its warm start is kept as it is.
//!
//! A body that no constraint touches skips all of this and takes one semi-implicit Euler step of
//! length `dt`, exactly as before the solver existed, so a world without contacts hashes as it did.

mod constraint;
mod joint;
mod stage;
mod warm;

use self::constraint::{
    Constraint, Ends, HERTZ_LIMIT_FRACTION, Link, Pass, STATIC_STIFFNESS_FACTOR, Setup, Softness,
    SolverBody, Staged, StepTerms, Surface, Sweep,
};
use self::joint::{JointConstraint, JointPass, JointSet, JointSetup, JointSweep, rigid_softness};
use self::stage::{ChunkOut, ColourJob, make_outputs, scatter};
use self::warm::WarmCache;
use crate::core::cells::{index_u32, put};
use crate::core::{
    Body, BodyKind, Colouring, ConstraintEnds, ContactManifold, ContactSolver, JointEntry, Motion,
    PhysicsError, SolverConfig, SolverExecution, SolverStats, StateHasher, StepFrame,
};
use crate::d2::{D2, Joint2};
use base::fpenv::FpGuard;
use std::sync::Mutex;

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
    constraints: Vec<Staged>,
    /// The buffers of the staged path, one per chunk of the largest possible colour; empty while
    /// the execution is the plain serial reference.
    outputs: Vec<Mutex<ChunkOut>>,
    /// The chunk size `outputs` was made for.
    outputs_chunk: usize,
    /// How the sweeps execute (serial scalar reference, lanes, scheduler).
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
    bodies: &'a [Option<Body<D2>>],
    entry: &ContactManifold<D2>,
) -> Option<(usize, usize, &'a Body<D2>, &'a Body<D2>)> {
    let (a, b) = (entry.first.index() as usize, entry.second.index() as usize);
    let first = bodies.get(a)?.as_ref()?;
    let second = bodies.get(b)?.as_ref()?;
    Some((a, b, first, second))
}

/// True for a body the step moves: dynamic and not asleep.
fn is_awake(body: &Body<D2>) -> bool {
    body.kind() == BodyKind::Dynamic && !body.is_asleep()
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

/// Runs `pass` on `constraint` with copies of its two bodies, which are stored back afterwards.
fn visit_constraint(bodies: &mut [SolverBody], constraint: &mut Constraint, pass: Pass) {
    let (Some(&a), Some(&b)) = (bodies.get(constraint.a), bodies.get(constraint.b)) else {
        return;
    };
    let (mut a, mut b) = (a, b);
    pass.apply(
        constraint,
        &mut Ends {
            a: &mut a,
            b: &mut b,
        },
    );
    // A static body is not written back: it did not change, and two constraints of one colour that
    // share it must not both write it (the order inside a colour must not matter).
    store(bodies, constraint.a, a);
    store(bodies, constraint.b, b);
}

/// Runs `pass` on the constraint `index` of the step, if there is one.
fn visit_member(constraints: &mut [Staged], bodies: &mut [SolverBody], index: u32, pass: Pass) {
    if let Some(staged) = constraints.get_mut(index as usize) {
        visit_constraint(bodies, &mut staged.constraint, pass);
    }
}

/// Runs `pass` on the constraints `members`, one after the other.
fn visit_members(
    constraints: &mut [Staged],
    bodies: &mut [SolverBody],
    members: &[u32],
    pass: Pass,
) {
    for &index in members {
        visit_member(constraints, bodies, index, pass);
    }
}

/// Stores `body` at `index` unless it is a static body.
fn store(bodies: &mut [SolverBody], index: usize, body: SolverBody) {
    if !body.is_static() {
        put(bodies, index, body);
    }
}

impl ContactSolver<D2> for Solver2 {
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
            bodies: vec![SolverBody::AT_REST; bodies],
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
        bodies: &mut [Option<Body<D2>>],
        contacts: &[ContactManifold<D2>],
        joints: &[JointEntry<D2>],
        frame: StepFrame<D2>,
    ) {
        // One deterministic floating-point state for the whole solve, whichever path runs; the
        // chunks take their own guard for the pool workers. Restores what it found.
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

impl Solver2 {
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
        if is_awake(body) {
            self.active.push(index);
        }
    }

    /// Makes the constraints of `contacts` in their order, with the warm start.
    fn prepare(
        &mut self,
        bodies: &[Option<Body<D2>>],
        contacts: &[ContactManifold<D2>],
        joints: &[JointEntry<D2>],
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
    fn add_joint(&mut self, context: &Prepare, entry: &JointEntry<D2>) {
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
            h: context.terms.h,
        };
        let mut joint = match entry.shape {
            Joint2::Revolute(shape) => JointConstraint::hinge(&setup, &shape),
            Joint2::Distance(shape) => JointConstraint::rod(&setup, &shape),
        };
        joint.solved = is_awake(first) || is_awake(second);
        self.joints.add(joint);
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
        self.colouring.push(ConstraintEnds {
            a: is_awake(first).then(|| index_u32(a)),
            b: is_awake(second).then(|| index_u32(b)),
        });
        let mut constraint = Constraint::new(link, &entry.manifold, &setup);
        if let Some(cached) = self.warm.find(cursor, entry.first, entry.second) {
            constraint.warm_from(cached);
        }
        self.constraints.push(Staged::new(constraint));
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
    fn each_active(&mut self, visit: impl Fn(&mut SolverBody)) {
        for &index in &self.active {
            visit_body(&mut self.bodies, index, &visit);
        }
    }

    /// Runs `pass` over the constraints that are solved, colour by colour (see the module
    /// comment). The reference visits them one by one; with an execution that asks for lanes or a
    /// scheduler each colour is solved as chunks (`stage`) and the overflow set follows serially.
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
        for &index in part {
            visit_member(&mut self.constraints, &mut self.bodies, index, pass);
        }
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
            simd: self.execution.simd,
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
    fn finish(&mut self, bodies: &mut [Option<Body<D2>>], frame: &StepFrame<D2>) {
        self.next.clear();
        self.joints.finish();
        for staged in &self.constraints {
            self.next.push(staged.constraint.cached());
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
        if body.kind() == BodyKind::Static || body.is_asleep() {
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
    use crate::core::{Colour, MAX_COLOURS};
    use crate::d2::{
        BodyDesc, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN,
        STANDARD_GRAVITY, Shape2, SleepConfig, World, WorldConfig,
    };
    use crate::test_support::Must;
    use base::math::Vec2;
    use base::pool::Handle;
    use base::rng::Pcg32;

    const DT: f32 = 1.0 / 60.0;

    #[test]
    fn a_solver_whose_impulses_are_not_numbers_is_not_finite() {
        let Ok(mut solver) = Solver2::new(SolverConfig::DEFAULT, 4, 4, 0) else {
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

    fn world_with(sleep: SleepConfig) -> World {
        World::new(WorldConfig {
            max_bodies: 64,
            gravity: STANDARD_GRAVITY,
            max_pairs: 1024,
            broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
            max_manifolds: 1024,
            max_joints: 0,
            speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
            solver: SolverConfig::DEFAULT,
            sleep,
        })
        .must("a world")
    }

    fn static_box(world: &mut World, x: f32, y: f32, hx: f32, hy: f32) {
        let desc = BodyDesc::new(
            BodyKind::Static,
            Vec2::new(x, y),
            Shape2::Box {
                half_extents: Vec2::new(hx, hy),
            },
        );
        world.create_body(&desc).must("a static box");
    }

    /// A random shape: a circle or a box of 0.2 to 0.5 m.
    fn random_shape(rng: &mut Pcg32) -> Shape2 {
        if rng.next_u32().is_multiple_of(2) {
            Shape2::Circle {
                radius: 0.25 + rng.next_f32() * 0.25,
            }
        } else {
            Shape2::Box {
                half_extents: Vec2::new(0.2 + rng.next_f32() * 0.3, 0.2 + rng.next_f32() * 0.3),
            }
        }
    }

    /// Drops one dynamic body of a random shape at a random place above the floor.
    fn drop_random_body(world: &mut World, rng: &mut Pcg32) {
        let x = rng.next_f32() * 6.0 - 3.0;
        let y = 0.5 + rng.next_f32() * 6.0;
        let shape = random_shape(rng);
        let desc = BodyDesc::new(BodyKind::Dynamic, Vec2::new(x, y), shape);
        world.create_body(&desc).must("a body");
    }

    /// A floor, two walls and `count` boxes and circles dropped at random places.
    fn random_scene(seed: u64, count: u32, sleep: SleepConfig) -> World {
        let mut world = world_with(sleep);
        let mut rng = Pcg32::new(seed, 11);
        static_box(&mut world, 0.0, -1.0, 8.0, 1.0);
        static_box(&mut world, -4.5, 4.0, 0.5, 6.0);
        static_box(&mut world, 4.5, 4.0, 0.5, 6.0);
        for _ in 0..count {
            drop_random_body(&mut world, &mut rng);
        }
        world
    }

    /// One row of a pyramid of unit boxes: `rows - row` boxes at height `row`.
    fn add_row(world: &mut World, row: u8, rows: u8) {
        let count = rows - row;
        for i in 0..count {
            let x = f32::from(i) - f32::from(count - 1) / 2.0;
            let shape = Shape2::Box {
                half_extents: Vec2::new(0.5, 0.5),
            };
            let desc = BodyDesc::new(BodyKind::Dynamic, Vec2::new(x, 0.5 + f32::from(row)), shape);
            world.create_body(&desc).must("box");
        }
    }

    fn pyramid(rows: u8) -> World {
        let mut world = world_with(SleepConfig::DEFAULT);
        static_box(&mut world, 0.0, -1.0, 20.0, 1.0);
        for row in 0..rows {
            add_row(&mut world, row, rows);
        }
        world
    }

    /// The bodies of constraint `index` that move, as solver slots.
    fn moving_ends(solver: &Solver2, index: u32) -> impl Iterator<Item = usize> {
        solver
            .constraints
            .get(index as usize)
            .into_iter()
            .flat_map(|staged| [staged.constraint.a, staged.constraint.b])
            .filter(|&slot| solver.bodies.get(slot).is_some_and(|b| !b.is_static()))
    }

    /// Checks colour `colour` of the last solve: the colour of each member, and no shared body.
    /// Counts each member in `listed`.
    fn check_colour(solver: &Solver2, colour: usize, listed: &mut [u32]) {
        let colouring = &solver.colouring;
        let expected = Colour::Index(u8::try_from(colour).must("a colour fits a byte"));
        for &index in colouring.members(colour) {
            *listed.get_mut(index as usize).must("an index") += 1;
            assert_eq!(colouring.colour_of(index as usize), expected);
        }
        let slots: Vec<usize> = colouring
            .members(colour)
            .iter()
            .flat_map(|&index| moving_ends(solver, index))
            .collect();
        let repeated = (0..slots.len()).any(|i| slots[..i].contains(&slots[i]));
        assert!(!repeated, "colour {colour} shares a body");
    }

    /// Checks the colouring the solver used in its last solve against the constraints it solved.
    fn check_colouring(solver: &Solver2) {
        let colouring = &solver.colouring;
        let mut listed = vec![0_u32; solver.constraints.len()];
        for colour in 0..colouring.colour_count() {
            check_colour(solver, colour, &mut listed);
        }
        for &index in colouring.overflow() {
            *listed.get_mut(index as usize).must("an index") += 1;
        }
        for (index, constraint) in solver.constraints.iter().enumerate() {
            let moves = [constraint.constraint.a, constraint.constraint.b]
                .into_iter()
                .any(|slot| solver.active.contains(&slot));
            assert_eq!(
                listed.get(index).copied(),
                Some(u32::from(moves)),
                "constraint {index}"
            );
        }
    }

    /// Steps a random scene and checks its colouring after every step.
    fn check_random_scene(seed: u64) {
        let mut world = random_scene(seed, 28, SleepConfig::DEFAULT);
        let mut busiest = 0;
        for _ in 0..150 {
            world.step(DT).must("a step");
            let stats = world.solver_stats();
            busiest = busiest.max(stats.solved);
            assert!(stats.colours <= MAX_COLOURS);
            check_colouring(world.solver_mut());
        }
        assert!(busiest > 5, "seed {seed}: the scene has too few contacts");
    }

    #[test]
    fn the_colouring_of_random_scenes_is_valid_and_complete_at_every_step() {
        for seed in 0..6 {
            check_random_scene(seed);
        }
    }

    #[test]
    fn the_colouring_of_a_pyramid_is_valid_and_uses_few_colours() {
        let mut world = pyramid(10);
        for _ in 0..20 {
            world.step(DT).must("a step");
            check_colouring(world.solver_mut());
        }
        let stats = world.solver_stats();
        assert_eq!(stats.overflow, 0);
        assert!(stats.colours >= 2 && stats.colours <= 12, "{stats:?}");
    }

    /// The state hash after each of `steps` steps with the constraints reordered inside the
    /// colours, and the solve order of each step.
    fn run(mut world: World, reorder: Option<Reorder>, steps: u32) -> (Vec<u64>, Vec<Vec<u32>>) {
        world.solver_mut().set_reorder(reorder);
        let mut hashes = Vec::new();
        let mut orders = Vec::new();
        for _ in 0..steps {
            world.step(DT).must("a step");
            hashes.push(world.state_hash());
            orders.push(world.solver_mut().colouring.order().to_vec());
        }
        (hashes, orders)
    }

    /// Runs `scene` in the canonical order and reordered three ways; the hashes must be equal.
    fn check_reordering(n: usize, scene: fn() -> World) {
        let (reference, plain) = run(scene(), None, 120);
        for reorder in [Reorder::Reverse, Reorder::Shuffle(1), Reorder::Shuffle(99)] {
            let (other, orders) = run(scene(), Some(reorder), 120);
            assert_eq!(reference, other, "scene {n}, {reorder:?}");
            // The reordering was real: the sweeps did visit the constraints in another order.
            let differs = plain.iter().zip(&orders).any(|(a, b)| a != b);
            assert!(differs, "scene {n}, {reorder:?}: nothing was reordered");
        }
    }

    #[test]
    fn the_order_inside_a_colour_does_not_change_a_single_bit() {
        let scenes: [fn() -> World; 3] = [
            || pyramid(8),
            || random_scene(1, 30, SleepConfig::DEFAULT),
            || random_scene(2, 30, SleepConfig::DISABLED),
        ];
        for (n, scene) in scenes.iter().enumerate() {
            check_reordering(n, *scene);
        }
    }

    /// Groups of four that took the lane path on this thread while `steps` steps ran.
    fn lane_groups_in(execution: SolverExecution, steps: u32) -> usize {
        let mut world = pyramid(10);
        world.set_execution(execution);
        let before = stage::LANE_GROUPS.with(std::cell::Cell::get);
        for _ in 0..steps {
            world.step(DT).must("a step");
        }
        stage::LANE_GROUPS.with(std::cell::Cell::get) - before
    }

    #[test]
    fn the_lane_path_runs_with_simd_on_and_never_with_it_off() {
        let simd = SolverExecution {
            simd: true,
            ..SolverExecution::SERIAL
        };
        assert!(lane_groups_in(simd, 3) > 0);
        assert_eq!(lane_groups_in(SolverExecution::SERIAL, 3), 0);
        let staged_scalar = SolverExecution {
            chunk: 4,
            ..SolverExecution::SERIAL
        };
        assert_eq!(lane_groups_in(staged_scalar, 3), 0);
    }
}
