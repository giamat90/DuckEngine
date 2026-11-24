//! The 3D instance of the generic physics core (phase 8, ADR-0018): `D3` fills the `Dim` trait with
//! `Vec3` positions, `Quat` orientations, a diagonal inverse inertia in the body frame, and the
//! shapes of [`Shape3`].
//!
//! Bodies, integration, mass properties, bounds, the state hash, the narrowphase for spheres,
//! capsules, boxes and convex hulls, the debug-draw port and the contact solver ([`Solver3`], card
//! 8-6: soft step, friction, warm start, restitution, sleeping; serial; the parallel solve is card 8-19)
//! and the joints ([`Joint3`], card 8-7: ball-and-socket, hinge with limits, distance) are complete.
//! The gyroscopic term is opt-in (card 8-26, [`SolverConfig::gyroscopic`], off by default). With it
//! off the integrator is semi-implicit Euler on the angular velocity: a torque-free body keeps its
//! angular velocity, which is not physical for a body whose principal moments differ (angular
//! momentum should be what is kept). With it on, each step (each sub-step of a body a constraint
//! touches) takes one implicit Newton step of Euler's equation in the body frame, so a box spun
//! about its middle axis tumbles. 2D has no such term.

mod aabb;
mod box_box;
mod clip;
mod debug_draw;
mod face_clip;
mod gjk;
mod gyroscopic;
mod hull;
mod joint;
mod manifold;
mod narrowphase;
mod shape;
mod solver;

use crate::core::{Dim, HashBits, MassProperties, Placed, Quantity, StateHasher};
use base::math::{Quat, Vec3};

pub use crate::core::{
    BodyKind, Colour, DEFAULT_BROADPHASE_MARGIN, DEFAULT_CHUNK, DEFAULT_CONTACT_DAMPING_RATIO,
    DEFAULT_CONTACT_HERTZ, DEFAULT_CONTACT_PUSH_SPEED, DEFAULT_FRICTION, DEFAULT_LINEAR_SLOP,
    DEFAULT_RELAX_ITERATIONS, DEFAULT_RESTITUTION, DEFAULT_RESTITUTION_THRESHOLD,
    DEFAULT_SLEEP_ANGULAR_SPEED, DEFAULT_SLEEP_LINEAR_SPEED, DEFAULT_SPECULATIVE_MARGIN,
    DEFAULT_SUB_STEPS, DEFAULT_TIME_TO_SLEEP, MAX_COLOURS, MAX_GIVEN_SPEED, MAX_GIVEN_SPIN,
    MAX_PAIRS, MAX_RELAX_ITERATIONS, MAX_SPEED, MAX_SPIN, MAX_SUB_STEPS, PhysicsError, SleepConfig,
    SolverConfig, SolverExecution, SolverStats, SpeedPolicy,
};
pub use aabb::Aabb3;
pub use debug_draw::{ARC_SEGMENTS, DebugColour, IPhysicsDebugDraw, NORMAL_LENGTH, draw_debug};
pub use hull::{Hull, MAX_HULL_VERTICES};
pub use joint::{AngleLimits, BallJoint3, DistanceJoint3, HingeJoint3, Joint3, LengthLimits};
pub use manifold::{ContactPoint3, MAX_MANIFOLD_POINTS, Manifold3};
pub use shape::Shape3;
pub use solver::Solver3;

/// The world of 3D bodies.
pub type World = crate::core::World<D3>;
/// Everything a 3D world is built from.
pub type WorldConfig = crate::core::WorldConfig<D3>;
/// The description a 3D body is created from.
pub type BodyDesc = crate::core::BodyDesc<D3>;
/// A 3D body.
pub type Body = crate::core::Body<D3>;
/// A pair of 3D bodies whose bounds overlap.
pub type Pair = crate::core::Pair<D3>;
/// The pairs of the last broadphase.
pub type PairList = crate::core::PairList<D3>;
/// A 3D shape at a position and orientation.
pub type Placed3 = Placed<D3>;
/// A 3D contact manifold with the two bodies it joins.
pub type ContactManifold = crate::core::ContactManifold<D3>;
/// The deepest overlap of the last step.
pub type Overlap = crate::core::Overlap<D3>;
/// The manifolds of a step.
pub type ManifoldList = crate::core::ManifoldList<D3>;
/// A handle to a 3D body.
pub type BodyHandle = crate::core::BodyHandle<D3>;
/// Names one joint of a 3D world.
pub type JointHandle = crate::core::JointHandle<D3>;
/// The description a 3D joint is created from.
pub type JointDesc = crate::core::JointDesc<D3>;
/// A live joint of a 3D world.
pub type JointEntry = crate::core::JointEntry<D3>;

/// Standard gravity, 9.80665 m/s^2 along -Y (right-handed, Y up).
pub const STANDARD_GRAVITY: Vec3 = Vec3::new(0.0, -9.806_65, 0.0);

/// The 3D dimension marker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct D3;

impl HashBits for Quat {
    fn write_bits(&self, hasher: &mut StateHasher) {
        hasher.write_f32(self.x);
        hasher.write_f32(self.y);
        hasher.write_f32(self.z);
        hasher.write_f32(self.w);
    }
}

/// How far the squared length of an orientation may be from one: a body's orientation is a rotation,
/// so a quaternion that is not close to unit length (the zero quaternion included) is refused where
/// the core checks finiteness, which is at creation and after every step.
const UNIT_LENGTH_TOLERANCE: f32 = 1.0e-4;

impl Quantity for Quat {
    fn is_finite(&self) -> bool {
        Quat::is_finite(*self) && (self.length_squared() - 1.0).abs() <= UNIT_LENGTH_TOLERANCE
    }
}

/// The inverse inertia tensor is diagonal in the body frame (the principal axes of all three shapes
/// coincide with the local axes): this is the world-space product `R diag(inv) R^T torque`.
fn world_inverse_inertia(rot: Quat, inv: Vec3, torque: Vec3) -> Vec3 {
    let local = rot.conjugate().rotate(torque);
    rot.rotate(Vec3::new(inv.x * local.x, inv.y * local.y, inv.z * local.z))
}

impl Dim for D3 {
    type Vec = Vec3;
    type Ang = Vec3;
    type Rot = Quat;
    type InvInertia = Vec3;
    type Shape = Shape3;
    type Bounds = Aabb3;
    type Manifold = Manifold3;
    type Solver = Solver3;
    type Joint = Joint3;

    fn apply_inverse_inertia(rot: Quat, inv_inertia: Vec3, torque: Vec3) -> Vec3 {
        world_inverse_inertia(rot, inv_inertia, torque)
    }

    /// `q' = normalise(q + dt/2 * (w, 0) * q)`: each step turns by `2 atan(|w| dt / 2)`, a little
    /// under `|w| dt`, and converges to the exact rotation as `dt` goes to zero; renormalised each
    /// step so the quaternion does not drift off unit length.
    fn advance_rotation(rot: Quat, angular_velocity: Vec3, dt: f32) -> Quat {
        let spin = Quat::new(
            angular_velocity.x,
            angular_velocity.y,
            angular_velocity.z,
            0.0,
        ) * rot;
        let half = 0.5 * dt;
        Quat::new(
            rot.x + spin.x * half,
            rot.y + spin.y * half,
            rot.z + spin.z * half,
            rot.w + spin.w * half,
        )
        .normalized()
        .unwrap_or(rot)
    }

    fn gyroscopic_spin(rot: Quat, inv_inertia: Vec3, spin: Vec3, dt: f32) -> Vec3 {
        gyroscopic::gyroscopic_spin(rot, inv_inertia, spin, dt)
    }

    fn overlap_depth(manifold: &Manifold3) -> f32 {
        manifold
            .active()
            .iter()
            .fold(0.0, |deepest, point| deepest.max(-point.separation))
    }

    fn shape_bounds(shape: &Shape3, position: Vec3, rot: Quat) -> Aabb3 {
        shape::bounds(shape, position, rot)
    }

    fn collide(a: &Placed<Self>, b: &Placed<Self>, margin: f32) -> Option<Manifold3> {
        narrowphase::collide(a, b, margin)
    }

    fn mass_properties(shape: &Shape3, density: f32) -> Result<MassProperties<Self>, PhysicsError> {
        shape::mass_properties(shape, density)
    }
}
