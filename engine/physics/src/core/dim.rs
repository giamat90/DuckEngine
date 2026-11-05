//! The narrow dimension trait. It names only what differs between dimensions: the types of position,
//! angular quantities, orientation, inverse inertia and shape, and three operations (inverse inertia
//! applied to a torque, the orientation update, mass properties of a shape). Everything else (storage,
//! handles, the step loop, the hash) is written once in the generic core.
//!
//! Rule kept from physics.md section 4: specialise rather than branch on the dimension; there is no
//! `if D == 2` anywhere in the core.

use super::bounds::Bounds;
use super::error::PhysicsError;
use super::hash::{HashBits, StateHasher};
use super::solver::ContactSolver;
use base::math::{Vec2, Vec3};
use std::fmt::Debug;
use std::ops::{Add, Mul};

/// A number-like value the world stores: it can be zero (`Default`), checked for finiteness and
/// hashed.
pub trait Quantity: Copy + Default + HashBits {
    /// True when every component is finite (neither NaN nor infinite).
    fn is_finite(&self) -> bool;
}

impl HashBits for f32 {
    fn write_bits(&self, hasher: &mut StateHasher) {
        hasher.write_f32(*self);
    }
}

impl Quantity for f32 {
    fn is_finite(&self) -> bool {
        f32::is_finite(*self)
    }
}

impl HashBits for Vec2 {
    fn write_bits(&self, hasher: &mut StateHasher) {
        hasher.write_f32(self.x);
        hasher.write_f32(self.y);
    }
}

impl Quantity for Vec2 {
    fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl HashBits for Vec3 {
    fn write_bits(&self, hasher: &mut StateHasher) {
        hasher.write_f32(self.x);
        hasher.write_f32(self.y);
        hasher.write_f32(self.z);
    }
}

impl Quantity for Vec3 {
    fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

/// What a shape and a density give a dynamic body.
pub struct MassProperties<D: Dim> {
    /// Mass in kilograms, finite and above zero.
    pub mass: f32,
    /// Inverse of the moment of inertia (about the centre of mass), finite.
    pub inv_inertia: D::InvInertia,
}

// Written by hand: `#[derive]` would add the bound `D: Copy` (ADR-0018, Rust friction 3).
impl<D: Dim> Clone for MassProperties<D> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<D: Dim> Copy for MassProperties<D> {}

/// A shape put in the world: what the narrowphase reads of a body.
pub struct Placed<D: Dim> {
    /// The shape, in the body frame.
    pub shape: D::Shape,
    /// Where the body's centre of mass is.
    pub position: D::Vec,
    /// How the body is turned.
    pub rotation: D::Rot,
}

// Written by hand: `#[derive]` would add the bound `D: Clone` (ADR-0018, Rust friction 3).
impl<D: Dim> Clone for Placed<D> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<D: Dim> Copy for Placed<D> {}

impl<D: Dim> Placed<D> {
    /// A shape at `position`, turned by `rotation`.
    #[must_use]
    pub const fn new(shape: D::Shape, position: D::Vec, rotation: D::Rot) -> Self {
        Self {
            shape,
            position,
            rotation,
        }
    }
}

/// Everything that depends on the dimension.
pub trait Dim: Sized {
    /// Position, velocity, force, gravity.
    type Vec: Quantity + Add<Output = Self::Vec> + Mul<f32, Output = Self::Vec>;
    /// Angular velocity and torque (a scalar in 2D, a vector in 3D).
    type Ang: Quantity + Add<Output = Self::Ang> + Mul<f32, Output = Self::Ang>;
    /// Orientation. Its default is the identity.
    type Rot: Quantity;
    /// Inverse moment of inertia.
    type InvInertia: Quantity;
    /// The shape descriptor, plain data.
    type Shape: Copy + HashBits;
    /// The axis-aligned box the broadphase stores.
    type Bounds: Bounds;
    /// What the narrowphase makes of two touching shapes: the contact normal and points.
    type Manifold: Copy + Debug + PartialEq + HashBits;
    /// The contact solver of this dimension ([`FreeSolver`](super::FreeSolver) answers no contact).
    type Solver: ContactSolver<Self>;

    /// The angular acceleration a `torque` gives a body of inverse inertia `inv_inertia` at
    /// orientation `rot`.
    fn apply_inverse_inertia(
        rot: Self::Rot,
        inv_inertia: Self::InvInertia,
        torque: Self::Ang,
    ) -> Self::Ang;

    /// The orientation after turning with `angular_velocity` for `dt` seconds. Must return a valid
    /// orientation (normalised) without calling `sin`, `cos` or `atan2`.
    fn advance_rotation(rot: Self::Rot, angular_velocity: Self::Ang, dt: f32) -> Self::Rot;

    /// The tightest axis-aligned box around `shape` placed at `position` with orientation `rot`.
    /// Not valid (see [`Bounds::is_valid`]) when an input is not finite or a coordinate overflows.
    fn shape_bounds(shape: &Self::Shape, position: Self::Vec, rot: Self::Rot) -> Self::Bounds;

    /// The contact manifold of two placed shapes, or `None` when they are further apart than
    /// `margin` (a speculative distance, zero or more) or the input is not valid (a shape out of
    /// range, a value not finite, an intermediate result that overflows). The normal points from
    /// `a` to `b`. Never returns a manifold with a value that is not finite.
    fn collide(a: &Placed<Self>, b: &Placed<Self>, margin: f32) -> Option<Self::Manifold>;

    /// Mass and inverse inertia of `shape` at `density` kilograms per unit of volume (per square
    /// metre in 2D).
    ///
    /// # Errors
    /// [`PhysicsError::InvalidShape`] when the shape is not valid or its mass is not representable.
    fn mass_properties(
        shape: &Self::Shape,
        density: f32,
    ) -> Result<MassProperties<Self>, PhysicsError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits_of<T: HashBits>(value: &T) -> u64 {
        let mut hasher = StateHasher::new();
        value.write_bits(&mut hasher);
        hasher.finish()
    }

    fn floats(values: &[f32]) -> u64 {
        let mut hasher = StateHasher::new();
        for &value in values {
            hasher.write_f32(value);
        }
        hasher.finish()
    }

    #[test]
    fn a_scalar_is_finite_unless_nan_or_infinite() {
        assert!(Quantity::is_finite(&1.0_f32));
        assert!(!Quantity::is_finite(&f32::NAN));
        assert!(!Quantity::is_finite(&f32::NEG_INFINITY));
    }

    #[test]
    fn a_vec2_is_finite_when_both_components_are() {
        assert!(Quantity::is_finite(&Vec2::new(1.0, 2.0)));
        assert!(!Quantity::is_finite(&Vec2::new(f32::NAN, 2.0)));
        assert!(!Quantity::is_finite(&Vec2::new(1.0, f32::INFINITY)));
    }

    #[test]
    fn a_vec3_is_finite_when_all_three_components_are() {
        assert!(Quantity::is_finite(&Vec3::new(1.0, 2.0, 3.0)));
        assert!(!Quantity::is_finite(&Vec3::new(f32::NAN, 2.0, 3.0)));
        assert!(!Quantity::is_finite(&Vec3::new(1.0, f32::INFINITY, 3.0)));
        assert!(!Quantity::is_finite(&Vec3::new(
            1.0,
            2.0,
            f32::NEG_INFINITY
        )));
    }

    #[test]
    fn vectors_hash_their_components_in_order() {
        assert_eq!(bits_of(&1.5_f32), floats(&[1.5]));
        assert_eq!(bits_of(&Vec2::new(1.0, 2.0)), floats(&[1.0, 2.0]));
        assert_eq!(bits_of(&Vec3::new(1.0, 2.0, 3.0)), floats(&[1.0, 2.0, 3.0]));
        assert_ne!(
            bits_of(&Vec3::new(1.0, 2.0, 3.0)),
            bits_of(&Vec3::new(1.0, 3.0, 2.0))
        );
    }
}
