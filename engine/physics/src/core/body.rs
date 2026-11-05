//! Body state, its description and the integrator.
//!
//! The integrator is semi-implicit (symplectic) Euler: velocity first, then position from the new
//! velocity. For constant acceleration `g` and step `dt` it gives `v_n = n g dt` and
//! `x_n = x_0 + g dt^2 n (n + 1) / 2`, which is the continuous `x_0 + g t^2 / 2` plus the documented
//! error `g t dt / 2`: first order in `dt`.

use super::dim::{Dim, Magnitude, Placed, Quantity};
use super::error::PhysicsError;
use super::hash::{HashBits, StateHasher};
use base::pool::Handle;
use std::marker::PhantomData;

/// The type a [`BodyHandle`] is a handle to; it carries the dimension so a 2D handle cannot be given
/// to a 3D world.
pub struct BodyTag<D>(PhantomData<fn() -> D>);

impl<D> BodyTag<D> {
    pub(super) const fn new() -> Self {
        Self(PhantomData)
    }
}

/// Names one body of a world: the slot index and the generation of that slot, eight bytes. A handle
/// of a destroyed body is detected as stale; a handle of another world is not (the slot may exist
/// there too).
pub type BodyHandle<D> = Handle<BodyTag<D>>;

/// Coulomb friction coefficient of a body that does not say otherwise: 0.6, the default of `Box2D` v3.
pub const DEFAULT_FRICTION: f32 = 0.6;

/// Restitution (bounciness) of a body that does not say otherwise: 0, no bounce.
pub const DEFAULT_RESTITUTION: f32 = 0.0;

/// Whether a body moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BodyKind {
    /// Never moves; infinite mass.
    Static,
    /// Moves under gravity and forces.
    Dynamic,
}

impl BodyKind {
    const TAG_STATIC: u8 = 0;
    const TAG_DYNAMIC: u8 = 1;

    const fn tag(self) -> u8 {
        match self {
            Self::Static => Self::TAG_STATIC,
            Self::Dynamic => Self::TAG_DYNAMIC,
        }
    }
}

/// The description a body is created from.
pub struct BodyDesc<D: Dim> {
    /// Static or dynamic.
    pub kind: BodyKind,
    /// Position of the centre of mass in metres.
    pub position: D::Vec,
    /// Orientation.
    pub rotation: D::Rot,
    /// Initial linear velocity in metres per second; ignored for a static body.
    pub linear_velocity: D::Vec,
    /// Initial angular velocity; ignored for a static body.
    pub angular_velocity: D::Ang,
    /// The shape.
    pub shape: D::Shape,
    /// Density in kilograms per unit of volume (per square metre in 2D); above zero. Only a
    /// dynamic body uses it.
    pub density: f32,
    /// Linear damping per second, zero or more: each step multiplies the velocity by
    /// `1 / (1 + dt * damping)`.
    pub linear_damping: f32,
    /// Angular damping per second, zero or more.
    pub angular_damping: f32,
    /// Coulomb friction coefficient, zero or more ([`DEFAULT_FRICTION`]). Two touching bodies use
    /// the square root of the product of their coefficients, so one frictionless body makes the
    /// contact frictionless.
    pub friction: f32,
    /// Restitution from zero (no bounce) to one (elastic) ([`DEFAULT_RESTITUTION`]). Two touching
    /// bodies use the larger of the two.
    pub restitution: f32,
}

// Written by hand: `#[derive]` would add the bound `D: Clone` (ADR-0018, Rust friction 3).
impl<D: Dim> Clone for BodyDesc<D> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<D: Dim> Copy for BodyDesc<D> {}

impl<D: Dim> BodyDesc<D> {
    /// A body of `kind` at `position` with `shape`: no rotation, at rest, density 1, no damping.
    #[must_use]
    pub fn new(kind: BodyKind, position: D::Vec, shape: D::Shape) -> Self {
        Self {
            kind,
            position,
            rotation: D::Rot::default(),
            linear_velocity: D::Vec::default(),
            angular_velocity: D::Ang::default(),
            shape,
            density: 1.0,
            linear_damping: 0.0,
            angular_damping: 0.0,
            friction: DEFAULT_FRICTION,
            restitution: DEFAULT_RESTITUTION,
        }
    }
}

/// Written before a non-default material in the state hash.
const MATERIAL_TAG: u8 = 0xA5;

/// Written for a sleeping body in the state hash, after the material and before the shape; an awake
/// body writes nothing, so a world in which nothing sleeps keeps the hash it had before sleeping
/// existed (card 4-14).
const SLEEP_TAG: u8 = 0x5A;

/// What a solver computed for a body in one step.
pub struct Motion<D: Dim> {
    /// Position of the centre of mass.
    pub position: D::Vec,
    /// Orientation.
    pub rotation: D::Rot,
    /// Linear velocity.
    pub linear_velocity: D::Vec,
    /// Angular velocity.
    pub angular_velocity: D::Ang,
}

/// The state of one body. Read it through the accessors; the world changes it.
pub struct Body<D: Dim> {
    kind: BodyKind,
    position: D::Vec,
    rotation: D::Rot,
    linear_velocity: D::Vec,
    angular_velocity: D::Ang,
    force: D::Vec,
    torque: D::Ang,
    mass: f32,
    inv_mass: f32,
    inv_inertia: D::InvInertia,
    linear_damping: f32,
    angular_damping: f32,
    friction: f32,
    restitution: f32,
    shape: D::Shape,
    /// True while the island of the body sleeps: the step does not move it and its velocities are
    /// zero. Only the world changes it.
    asleep: bool,
}

impl<D: Dim> Clone for Body<D> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<D: Dim> Copy for Body<D> {}

/// The factor a velocity is multiplied by in one step of length `dt` under `damping` per second.
pub(crate) fn damping_factor(damping: f32, dt: f32) -> f32 {
    1.0 / (1.0 + dt * damping)
}

impl<D: Dim> Body<D> {
    /// Checks `desc` and computes the mass properties.
    pub(super) fn from_desc(desc: &BodyDesc<D>) -> Result<Self, PhysicsError> {
        let finite = desc.position.is_finite()
            && desc.rotation.is_finite()
            && desc.linear_velocity.is_finite()
            && desc.angular_velocity.is_finite()
            && desc.linear_damping.is_finite()
            && desc.angular_damping.is_finite()
            && desc.friction.is_finite()
            && desc.restitution.is_finite();
        if !finite {
            return Err(PhysicsError::NotFinite);
        }
        if desc.linear_damping < 0.0 || desc.angular_damping < 0.0 {
            return Err(PhysicsError::InvalidShape);
        }
        if desc.friction < 0.0 || !(0.0..=1.0).contains(&desc.restitution) {
            return Err(PhysicsError::InvalidMaterial);
        }
        match desc.kind {
            BodyKind::Static => Self::new_static(desc),
            BodyKind::Dynamic => Self::new_dynamic(desc),
        }
    }

    fn new_static(desc: &BodyDesc<D>) -> Result<Self, PhysicsError> {
        // The shape is checked at unit density: a static body has no mass of its own.
        D::mass_properties(&desc.shape, 1.0)?;
        Ok(Self {
            kind: BodyKind::Static,
            position: desc.position,
            rotation: desc.rotation,
            linear_velocity: D::Vec::default(),
            angular_velocity: D::Ang::default(),
            force: D::Vec::default(),
            torque: D::Ang::default(),
            mass: 0.0,
            inv_mass: 0.0,
            inv_inertia: D::InvInertia::default(),
            linear_damping: desc.linear_damping,
            angular_damping: desc.angular_damping,
            friction: desc.friction,
            restitution: desc.restitution,
            shape: desc.shape,
            asleep: false,
        })
    }

    fn new_dynamic(desc: &BodyDesc<D>) -> Result<Self, PhysicsError> {
        let properties = D::mass_properties(&desc.shape, desc.density)?;
        let inv_mass = 1.0 / properties.mass;
        if !inv_mass.is_finite() || !properties.inv_inertia.is_finite() {
            return Err(PhysicsError::InvalidShape);
        }
        Ok(Self {
            kind: BodyKind::Dynamic,
            position: desc.position,
            rotation: desc.rotation,
            linear_velocity: desc.linear_velocity,
            angular_velocity: desc.angular_velocity,
            force: D::Vec::default(),
            torque: D::Ang::default(),
            mass: properties.mass,
            inv_mass,
            inv_inertia: properties.inv_inertia,
            linear_damping: desc.linear_damping,
            angular_damping: desc.angular_damping,
            friction: desc.friction,
            restitution: desc.restitution,
            shape: desc.shape,
            asleep: false,
        })
    }

    /// Static or dynamic.
    #[must_use]
    pub fn kind(&self) -> BodyKind {
        self.kind
    }

    /// Position of the centre of mass.
    #[must_use]
    pub fn position(&self) -> D::Vec {
        self.position
    }

    /// Orientation.
    #[must_use]
    pub fn rotation(&self) -> D::Rot {
        self.rotation
    }

    /// Linear velocity.
    #[must_use]
    pub fn linear_velocity(&self) -> D::Vec {
        self.linear_velocity
    }

    /// Angular velocity.
    #[must_use]
    pub fn angular_velocity(&self) -> D::Ang {
        self.angular_velocity
    }

    /// Mass in kilograms; zero for a static body (read it with [`Body::inverse_mass`] for use in
    /// formulas: infinite mass is an inverse mass of zero).
    #[must_use]
    pub fn mass(&self) -> f32 {
        self.mass
    }

    /// Inverse mass; zero for a static body.
    #[must_use]
    pub fn inverse_mass(&self) -> f32 {
        self.inv_mass
    }

    /// Inverse moment of inertia; zero for a static body.
    #[must_use]
    pub fn inverse_inertia(&self) -> D::InvInertia {
        self.inv_inertia
    }

    /// Coulomb friction coefficient.
    #[must_use]
    pub fn friction(&self) -> f32 {
        self.friction
    }

    /// Restitution from zero to one.
    #[must_use]
    pub fn restitution(&self) -> f32 {
        self.restitution
    }

    /// True while the body sleeps (see [`SleepConfig`](super::sleep::SleepConfig)): the step leaves
    /// it where it is until something wakes it. Always false for a static body.
    #[must_use]
    pub fn is_asleep(&self) -> bool {
        self.asleep
    }

    /// The shape.
    #[must_use]
    pub fn shape(&self) -> D::Shape {
        self.shape
    }

    /// The axis-aligned box around the shape at the body's position and orientation.
    #[must_use]
    pub fn bounds(&self) -> D::Bounds {
        D::shape_bounds(&self.shape, self.position, self.rotation)
    }

    /// The shape at the body's position and orientation, as the narrowphase reads it.
    #[must_use]
    pub fn placed(&self) -> Placed<D> {
        Placed::new(self.shape, self.position, self.rotation)
    }

    /// Sets the linear velocity.
    pub(super) fn set_linear_velocity(&mut self, velocity: D::Vec) -> Result<(), PhysicsError> {
        self.check_motion(velocity.is_finite())?;
        self.linear_velocity = velocity;
        Ok(())
    }

    /// Sets the angular velocity.
    pub(super) fn set_angular_velocity(&mut self, velocity: D::Ang) -> Result<(), PhysicsError> {
        self.check_motion(velocity.is_finite())?;
        self.angular_velocity = velocity;
        Ok(())
    }

    /// Adds `force` to what acts on the body in the next step only.
    pub(super) fn apply_force(&mut self, force: D::Vec) -> Result<(), PhysicsError> {
        self.check_motion(force.is_finite())?;
        let sum = self.force + force;
        if !sum.is_finite() {
            return Err(PhysicsError::NotFinite);
        }
        self.force = sum;
        Ok(())
    }

    /// Adds `torque` to what acts on the body in the next step only.
    pub(super) fn apply_torque(&mut self, torque: D::Ang) -> Result<(), PhysicsError> {
        self.check_motion(torque.is_finite())?;
        let sum = self.torque + torque;
        if !sum.is_finite() {
            return Err(PhysicsError::NotFinite);
        }
        self.torque = sum;
        Ok(())
    }

    /// True when both velocities are at or below the limits (`linear` in metres per second,
    /// `angular` in the unit of the angular velocity per second).
    pub(crate) fn rests_below(&self, linear: f32, angular: f32) -> bool {
        self.linear_velocity.length_squared() <= linear * linear
            && self.angular_velocity.length_squared() <= angular * angular
    }

    /// Puts a dynamic body to sleep: both velocities become zero, so a sleeping body is at rest.
    pub(crate) fn fall_asleep(&mut self) {
        if self.kind == BodyKind::Dynamic {
            self.asleep = true;
            self.linear_velocity = D::Vec::default();
            self.angular_velocity = D::Ang::default();
        }
    }

    /// Wakes the body; the velocities stay zero until the step moves it.
    pub(crate) fn wake_up(&mut self) {
        self.asleep = false;
    }

    /// A static body does not take motion; a value that is not finite is refused.
    fn check_motion(&self, finite: bool) -> Result<(), PhysicsError> {
        if self.kind == BodyKind::Static {
            Err(PhysicsError::StaticBody)
        } else if finite {
            Ok(())
        } else {
            Err(PhysicsError::NotFinite)
        }
    }

    /// One semi-implicit Euler step of length `dt` under `gravity`; clears the force and the torque.
    /// A static body and a sleeping body stay where they are.
    pub(crate) fn integrate(&mut self, gravity: D::Vec, dt: f32) {
        if self.kind == BodyKind::Static || self.asleep {
            return;
        }
        let acceleration = gravity + self.force * self.inv_mass;
        let velocity =
            (self.linear_velocity + acceleration * dt) * damping_factor(self.linear_damping, dt);
        self.linear_velocity = velocity;
        self.position = self.position + velocity * dt;

        let angular_acceleration =
            D::apply_inverse_inertia(self.rotation, self.inv_inertia, self.torque);
        let spin = (self.angular_velocity + angular_acceleration * dt)
            * damping_factor(self.angular_damping, dt);
        self.angular_velocity = spin;
        self.rotation = D::advance_rotation(self.rotation, spin, dt);

        self.force = D::Vec::default();
        self.torque = D::Ang::default();
    }

    /// The force that acts in the next step.
    pub(crate) fn force(&self) -> D::Vec {
        self.force
    }

    /// The torque that acts in the next step.
    pub(crate) fn torque(&self) -> D::Ang {
        self.torque
    }

    /// Linear damping per second.
    pub(crate) fn linear_damping(&self) -> f32 {
        self.linear_damping
    }

    /// Angular damping per second.
    pub(crate) fn angular_damping(&self) -> f32 {
        self.angular_damping
    }

    /// Replaces the motion a solver computed and clears the force and the torque (they were used
    /// by that step). A static body is left alone.
    pub(crate) fn set_motion(&mut self, motion: &Motion<D>) {
        if self.kind == BodyKind::Static {
            return;
        }
        self.position = motion.position;
        self.rotation = motion.rotation;
        self.linear_velocity = motion.linear_velocity;
        self.angular_velocity = motion.angular_velocity;
        self.force = D::Vec::default();
        self.torque = D::Ang::default();
    }

    /// True when position, rotation and both velocities are finite; checked on the result of an
    /// integration before it is committed.
    pub(super) fn motion_is_finite(&self) -> bool {
        self.position.is_finite()
            && self.rotation.is_finite()
            && self.linear_velocity.is_finite()
            && self.angular_velocity.is_finite()
    }

    /// Writes every field that decides the future of the body, in a fixed order. `mass` is not
    /// written: it is `1 / inv_mass` for a dynamic body and zero for a static one.
    pub(super) fn write_bits(&self, hasher: &mut StateHasher) {
        hasher.write_u8(self.kind.tag());
        self.position.write_bits(hasher);
        self.rotation.write_bits(hasher);
        self.linear_velocity.write_bits(hasher);
        self.angular_velocity.write_bits(hasher);
        self.force.write_bits(hasher);
        self.torque.write_bits(hasher);
        hasher.write_f32(self.inv_mass);
        self.inv_inertia.write_bits(hasher);
        hasher.write_f32(self.linear_damping);
        hasher.write_f32(self.angular_damping);
        // The material is written only when it is not the default, so a world made before the
        // solver existed (card 4-13), or one that never sets a material, keeps its hash.
        if self.friction.to_bits() != DEFAULT_FRICTION.to_bits()
            || self.restitution.to_bits() != DEFAULT_RESTITUTION.to_bits()
        {
            hasher.write_u8(MATERIAL_TAG);
            hasher.write_f32(self.friction);
            hasher.write_f32(self.restitution);
        }
        if self.asleep {
            hasher.write_u8(SLEEP_TAG);
        }
        self.shape.write_bits(hasher);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Must;

    #[test]
    fn the_kind_tags_are_distinct_and_fixed() {
        assert_eq!(BodyKind::Static.tag(), 0);
        assert_eq!(BodyKind::Dynamic.tag(), 1);
    }

    #[test]
    fn a_body_is_placed_with_its_shape_position_and_rotation() {
        use crate::d2::{D2, Rot2, Shape2};
        use base::math::Vec2;
        let shape = Shape2::Circle { radius: 0.5 };
        let mut desc = BodyDesc::<D2>::new(BodyKind::Dynamic, Vec2::new(1.0, 2.0), shape);
        desc.rotation = Rot2::from_angle(0.5);
        let body = Body::from_desc(&desc).must("a valid body");
        let placed = body.placed();
        assert_eq!(placed.shape, shape);
        assert_eq!(placed.position, Vec2::new(1.0, 2.0));
        assert_eq!(placed.rotation, desc.rotation);
    }

    fn ball_with(friction: f32, restitution: f32) -> Result<Body<crate::d2::D2>, PhysicsError> {
        use crate::d2::{D2, Shape2};
        use base::math::Vec2;
        let mut desc = BodyDesc::<D2>::new(
            BodyKind::Dynamic,
            Vec2::new(0.0, 0.0),
            Shape2::Circle { radius: 0.5 },
        );
        desc.friction = friction;
        desc.restitution = restitution;
        Body::from_desc(&desc)
    }

    fn resting_ball(kind: BodyKind) -> Body<crate::d2::D2> {
        use crate::d2::{D2, Shape2};
        use base::math::Vec2;
        let desc = BodyDesc::<D2>::new(kind, Vec2::new(0.0, 0.0), Shape2::Circle { radius: 0.5 });
        Body::from_desc(&desc).must("a valid body")
    }

    #[test]
    fn a_body_rests_when_both_speeds_are_at_or_below_the_limits() {
        use base::math::Vec2;
        let mut body = resting_ball(BodyKind::Dynamic);
        body.set_linear_velocity(Vec2::new(0.03, 0.04))
            .must("velocity");
        assert!(
            body.rests_below(0.05, 1.0),
            "speed exactly at the limit rests"
        );
        assert!(
            !body.rests_below(0.0499, 1.0),
            "linear speed above the limit"
        );
        body.set_angular_velocity(0.5).must("spin");
        assert!(
            body.rests_below(0.05, 0.5),
            "spin exactly at the limit rests"
        );
        assert!(
            !body.rests_below(0.05, 0.4),
            "angular speed above the limit"
        );
    }

    #[test]
    fn a_dynamic_body_falls_asleep_with_zero_velocity_and_wakes_up() {
        use base::math::Vec2;
        let mut body = resting_ball(BodyKind::Dynamic);
        body.set_linear_velocity(Vec2::new(0.01, 0.0))
            .must("velocity");
        body.set_angular_velocity(0.01).must("spin");
        assert!(!body.is_asleep());
        body.fall_asleep();
        assert!(body.is_asleep());
        assert_eq!(body.linear_velocity(), Vec2::ZERO);
        assert_eq!(body.angular_velocity().to_bits(), 0.0_f32.to_bits());
        body.wake_up();
        assert!(!body.is_asleep());
    }

    #[test]
    fn a_static_body_never_falls_asleep() {
        let mut body = resting_ball(BodyKind::Static);
        body.fall_asleep();
        assert!(!body.is_asleep());
    }

    #[test]
    fn a_sleeping_body_is_not_integrated_and_an_awake_one_is() {
        use base::math::Vec2;
        let mut sleeper = resting_ball(BodyKind::Dynamic);
        sleeper.fall_asleep();
        let before = sleeper;
        sleeper.integrate(Vec2::new(0.0, -9.81), 1.0 / 60.0);
        assert_eq!(hash_of_body(&sleeper), hash_of_body(&before));
        let mut awake = resting_ball(BodyKind::Dynamic);
        awake.integrate(Vec2::new(0.0, -9.81), 1.0 / 60.0);
        assert!(awake.linear_velocity().y < 0.0);
    }

    #[test]
    fn the_sleep_flag_is_part_of_the_body_hash() {
        let awake = resting_ball(BodyKind::Dynamic);
        let mut asleep = awake;
        asleep.fall_asleep();
        assert_ne!(hash_of_body(&awake), hash_of_body(&asleep));
        asleep.wake_up();
        assert_eq!(hash_of_body(&awake), hash_of_body(&asleep));
    }

    fn hash_of_body(body: &Body<crate::d2::D2>) -> u64 {
        let mut hasher = StateHasher::new();
        body.write_bits(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn each_material_value_is_checked_on_its_own() {
        assert!(ball_with(0.0, 0.0).is_ok());
        assert!(ball_with(2.0, 1.0).is_ok());
        assert_eq!(
            ball_with(-0.1, 0.5).err(),
            Some(PhysicsError::InvalidMaterial)
        );
        assert_eq!(
            ball_with(0.5, 1.1).err(),
            Some(PhysicsError::InvalidMaterial)
        );
        assert_eq!(
            ball_with(0.5, -0.1).err(),
            Some(PhysicsError::InvalidMaterial)
        );
        assert_eq!(
            ball_with(f32::NAN, 0.5).err(),
            Some(PhysicsError::NotFinite)
        );
        assert_eq!(
            ball_with(0.5, f32::INFINITY).err(),
            Some(PhysicsError::NotFinite)
        );
    }

    #[test]
    fn a_material_changes_the_hash_whichever_of_its_two_values_is_not_the_default() {
        let plain = hash_of_body(&ball_with(DEFAULT_FRICTION, DEFAULT_RESTITUTION).must("body"));
        let friction = hash_of_body(&ball_with(0.2, DEFAULT_RESTITUTION).must("body"));
        let restitution = hash_of_body(&ball_with(DEFAULT_FRICTION, 0.3).must("body"));
        let both = hash_of_body(&ball_with(0.2, 0.3).must("body"));
        let all = [plain, friction, restitution, both];
        let distinct = (0..all.len()).all(|i| (i + 1..all.len()).all(|j| all[i] != all[j]));
        assert!(distinct);
    }

    #[test]
    fn the_force_and_torque_of_the_next_step_can_be_read_back() {
        use base::math::Vec2;
        let mut body = ball_with(0.5, 0.0).must("body");
        assert_eq!(body.apply_force(Vec2::new(1.0, -2.0)), Ok(()));
        assert_eq!(body.apply_torque(3.0), Ok(()));
        assert_eq!(body.force(), Vec2::new(1.0, -2.0));
        assert_eq!(body.torque().to_bits(), 3.0_f32.to_bits());
    }
}
