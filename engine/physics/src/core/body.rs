//! Body state, its description and the integrator.
//!
//! The integrator is semi-implicit (symplectic) Euler: velocity first, then position from the new
//! velocity. For constant acceleration `g` and step `dt` it gives `v_n = n g dt` and
//! `x_n = x_0 + g dt^2 n (n + 1) / 2`, which is the continuous `x_0 + g t^2 / 2` plus the documented
//! error `g t dt / 2`: first order in `dt`.

use super::dim::{Dim, Quantity};
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
        }
    }
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
    shape: D::Shape,
}

impl<D: Dim> Clone for Body<D> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<D: Dim> Copy for Body<D> {}

/// The factor a velocity is multiplied by in one step of length `dt` under `damping` per second.
fn damping_factor(damping: f32, dt: f32) -> f32 {
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
            && desc.angular_damping.is_finite();
        if !finite {
            return Err(PhysicsError::NotFinite);
        }
        if desc.linear_damping < 0.0 || desc.angular_damping < 0.0 {
            return Err(PhysicsError::InvalidShape);
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
            shape: desc.shape,
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
            shape: desc.shape,
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

    /// The shape.
    #[must_use]
    pub fn shape(&self) -> D::Shape {
        self.shape
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
    /// A static body stays where it is.
    pub(super) fn integrate(&mut self, gravity: D::Vec, dt: f32) {
        if self.kind == BodyKind::Static {
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
        self.shape.write_bits(hasher);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kind_tags_are_distinct_and_fixed() {
        assert_eq!(BodyKind::Static.tag(), 0);
        assert_eq!(BodyKind::Dynamic.tag(), 1);
    }
}
