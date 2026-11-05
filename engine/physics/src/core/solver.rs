//! The solver seam of the core: the tunables of the contact solver ([`SolverConfig`]), the trait
//! the instance implements ([`ContactSolver`]) and the solver of a world without contact response
//! ([`FreeSolver`]).
//!
//! The contact solver of the 2D instance follows the "soft step" design of `Box2D` v3 (owner choice
//! O2 of `wiki/architecture/physics-and-jobs-v0.md`): soft contact constraints, a few sub-steps per
//! step instead of iteration counts, a relax pass in every sub-step and restitution applied once
//! after the last sub-step. The constraint math is dimension specific and lives in the instance.

use super::body::Body;
use super::dim::Dim;
use super::error::PhysicsError;
use super::hash::StateHasher;
use super::manifolds::ContactManifold;

/// The most sub-steps one step may be divided into (see [`SolverConfig::sub_steps`]).
pub const MAX_SUB_STEPS: u32 = 16;

/// The most relax passes per sub-step (see [`SolverConfig::relax_iterations`]).
pub const MAX_RELAX_ITERATIONS: u32 = 4;

/// Sub-steps per step: 4, the "4 sub-steps, 1 relax" of workload W1 (owner choice O9).
pub const DEFAULT_SUB_STEPS: u32 = 4;

/// Relax passes per sub-step: 1.
pub const DEFAULT_RELAX_ITERATIONS: u32 = 1;

/// Stiffness of a contact in hertz: 30, the value of `Box2D` v3 **[K]**. A contact against a static
/// body uses twice this.
pub const DEFAULT_CONTACT_HERTZ: f32 = 30.0;

/// Damping ratio of a contact: 10 (heavily over-damped, no bounce from the soft constraint), the
/// value of `Box2D` v3 **[K]**.
pub const DEFAULT_CONTACT_DAMPING_RATIO: f32 = 10.0;

/// The fastest a contact pushes two overlapping shapes apart, in metres per second: 3, the value of
/// `Box2D` v3 **[K]**. It keeps a deep overlap from throwing the bodies away.
pub const DEFAULT_CONTACT_PUSH_SPEED: f32 = 3.0;

/// A body must approach faster than this, in metres per second, to bounce: 1, the value of `Box2D`
/// v3 **[K]**. Slower approaches are inelastic, so a resting body does not jitter.
pub const DEFAULT_RESTITUTION_THRESHOLD: f32 = 1.0;

/// The overlap, in metres, a contact tolerates without pushing: 1 millimetre. `Box2D` v3 uses 5
/// millimetres **[K]**, but every level of a stack rests about 1.5 slops into the one below, and
/// the checkpoint measurement of card 4-13 (`reviews/4-13.md`) shows that with 5 millimetres a
/// pyramid ten boxes high sinks by 0.059 m at the top, over the 0.05 m a settled stack may move,
/// against 0.017 m with 1 millimetre. The slop only has to be above the floating-point noise of the
/// manifold (about 1e-6 m) to keep resting contacts from switching on and off.
pub const DEFAULT_LINEAR_SLOP: f32 = 0.001;

/// The tunables of the contact solver. All are fixed when the world is built.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolverConfig {
    /// How many sub-steps one step is divided into, from 1 to [`MAX_SUB_STEPS`]
    /// ([`DEFAULT_SUB_STEPS`]).
    pub sub_steps: u32,
    /// How many relax passes follow each sub-step, from 0 to [`MAX_RELAX_ITERATIONS`]
    /// ([`DEFAULT_RELAX_ITERATIONS`]).
    pub relax_iterations: u32,
    /// Stiffness of a contact in hertz, above zero ([`DEFAULT_CONTACT_HERTZ`]). It is capped at a
    /// quarter of the sub-step rate, below which the soft constraint is stable.
    pub contact_hertz: f32,
    /// Damping ratio of a contact, zero or more ([`DEFAULT_CONTACT_DAMPING_RATIO`]).
    pub contact_damping_ratio: f32,
    /// The fastest a contact pushes overlapping shapes apart, in metres per second, zero or more
    /// ([`DEFAULT_CONTACT_PUSH_SPEED`]).
    pub contact_push_speed: f32,
    /// The approach speed, in metres per second, above which a contact bounces, zero or more
    /// ([`DEFAULT_RESTITUTION_THRESHOLD`]).
    pub restitution_threshold: f32,
    /// The overlap, in metres, that is not pushed apart, zero or more ([`DEFAULT_LINEAR_SLOP`]).
    pub linear_slop: f32,
}

impl SolverConfig {
    /// The documented defaults.
    pub const DEFAULT: Self = Self {
        sub_steps: DEFAULT_SUB_STEPS,
        relax_iterations: DEFAULT_RELAX_ITERATIONS,
        contact_hertz: DEFAULT_CONTACT_HERTZ,
        contact_damping_ratio: DEFAULT_CONTACT_DAMPING_RATIO,
        contact_push_speed: DEFAULT_CONTACT_PUSH_SPEED,
        restitution_threshold: DEFAULT_RESTITUTION_THRESHOLD,
        linear_slop: DEFAULT_LINEAR_SLOP,
    };

    /// Checks every field against its range.
    ///
    /// # Errors
    /// [`PhysicsError::InvalidSolverConfig`] when a count is out of range or a number is not finite
    /// or outside its range.
    pub fn validate(&self) -> Result<(), PhysicsError> {
        let counts = (1..=MAX_SUB_STEPS).contains(&self.sub_steps)
            && self.relax_iterations <= MAX_RELAX_ITERATIONS;
        let numbers = self.contact_hertz.is_finite()
            && self.contact_hertz > 0.0
            && is_zero_or_more(self.contact_damping_ratio)
            && is_zero_or_more(self.contact_push_speed)
            && is_zero_or_more(self.restitution_threshold)
            && is_zero_or_more(self.linear_slop);
        if counts && numbers {
            Ok(())
        } else {
            Err(PhysicsError::InvalidSolverConfig)
        }
    }
}

impl Default for SolverConfig {
    fn default() -> Self {
        Self::DEFAULT
    }
}

fn is_zero_or_more(value: f32) -> bool {
    value.is_finite() && value >= 0.0
}

/// What a solver needs to know of the step besides the bodies and contacts.
pub struct StepFrame<D: Dim> {
    /// Acceleration of gravity.
    pub gravity: D::Vec,
    /// Length of the step in seconds, finite and above zero.
    pub dt: f32,
}

/// Moves the bodies of one step and answers their contacts. One value lives in each world.
pub trait ContactSolver<D: Dim>: Sized {
    /// A solver for a world of `max_bodies` bodies and `max_manifolds` manifolds. Allocates all it
    /// needs now; nothing allocates later.
    ///
    /// # Errors
    /// [`PhysicsError::InvalidSolverConfig`] when `config` is not valid.
    fn new(config: SolverConfig, max_bodies: u32, max_manifolds: u32)
    -> Result<Self, PhysicsError>;

    /// Advances the copies in `bodies` (indexed by slot, `None` for a free slot) by one step under
    /// `contacts`, which are the manifolds of the bodies as they are. Forces and torques are used
    /// and cleared. The result is not committed: it becomes the warm start of the next step only in
    /// [`ContactSolver::commit`], so a step that is refused can be dropped.
    fn solve(
        &mut self,
        bodies: &mut [Option<Body<D>>],
        contacts: &[ContactManifold<D>],
        frame: StepFrame<D>,
    );

    /// True when everything the last [`ContactSolver::solve`] kept for the next step is finite.
    fn is_finite(&self) -> bool;

    /// Keeps what the last [`ContactSolver::solve`] made for the next step.
    fn commit(&mut self);

    /// Writes what the solver carries from step to step (the warm start) into `hasher`, nothing
    /// when it carries nothing.
    fn write_bits(&self, hasher: &mut StateHasher);
}

/// A solver that answers no contact: every body moves freely by one step of semi-implicit Euler.
/// It is the solver of the test instance of the generic core and the reference for a world in which
/// nothing touches.
pub struct FreeSolver;

impl<D: Dim> ContactSolver<D> for FreeSolver {
    fn new(
        config: SolverConfig,
        _max_bodies: u32,
        _max_manifolds: u32,
    ) -> Result<Self, PhysicsError> {
        config.validate()?;
        Ok(Self)
    }

    fn solve(
        &mut self,
        bodies: &mut [Option<Body<D>>],
        _contacts: &[ContactManifold<D>],
        frame: StepFrame<D>,
    ) {
        for body in bodies.iter_mut().flatten() {
            body.integrate(frame.gravity, frame.dt);
        }
    }

    fn is_finite(&self) -> bool {
        true
    }

    fn commit(&mut self) {}

    fn write_bits(&self, _hasher: &mut StateHasher) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invalid(change: impl FnOnce(&mut SolverConfig)) -> bool {
        let mut config = SolverConfig::DEFAULT;
        change(&mut config);
        config.validate() == Err(PhysicsError::InvalidSolverConfig)
    }

    #[test]
    fn the_defaults_are_valid_and_documented_values() {
        let config = SolverConfig::default();
        assert_eq!(config, SolverConfig::DEFAULT);
        assert_eq!(config.validate(), Ok(()));
        assert_eq!((config.sub_steps, config.relax_iterations), (4, 1));
        assert_eq!(
            (config.contact_hertz, config.contact_damping_ratio),
            (30.0, 10.0)
        );
        assert_eq!(
            (config.contact_push_speed, config.restitution_threshold),
            (3.0, 1.0)
        );
        assert_eq!(config.linear_slop.to_bits(), 0.001_f32.to_bits());
    }

    #[test]
    fn the_counts_have_their_ranges() {
        assert!(invalid(|c| c.sub_steps = 0));
        assert!(invalid(|c| c.sub_steps = MAX_SUB_STEPS + 1));
        assert!(!invalid(|c| c.sub_steps = MAX_SUB_STEPS));
        assert!(!invalid(|c| c.sub_steps = 1));
        assert!(invalid(|c| c.relax_iterations = MAX_RELAX_ITERATIONS + 1));
        assert!(!invalid(|c| c.relax_iterations = MAX_RELAX_ITERATIONS));
        assert!(!invalid(|c| c.relax_iterations = 0));
    }

    #[test]
    fn the_numbers_must_be_finite_and_in_range() {
        assert!(invalid(|c| c.contact_hertz = 0.0));
        assert!(invalid(|c| c.contact_hertz = -1.0));
        assert!(invalid(|c| c.contact_hertz = f32::NAN));
        assert!(invalid(|c| c.contact_hertz = f32::INFINITY));
        assert!(invalid(|c| c.contact_damping_ratio = -0.1));
        assert!(invalid(|c| c.contact_damping_ratio = f32::NAN));
        assert!(!invalid(|c| c.contact_damping_ratio = 0.0));
        assert!(invalid(|c| c.contact_push_speed = -0.1));
        assert!(invalid(|c| c.contact_push_speed = f32::INFINITY));
        assert!(!invalid(|c| c.contact_push_speed = 0.0));
        assert!(invalid(|c| c.restitution_threshold = -0.1));
        assert!(invalid(|c| c.restitution_threshold = f32::NAN));
        assert!(!invalid(|c| c.restitution_threshold = 0.0));
        assert!(invalid(|c| c.linear_slop = -0.1));
        assert!(invalid(|c| c.linear_slop = f32::NAN));
        assert!(!invalid(|c| c.linear_slop = 0.0));
    }
}
