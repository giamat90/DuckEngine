//! The 2D instance of the core: the [`D2`] marker, position and velocity as `Vec2`, angular
//! quantities as `f32` (counter-clockwise, radians per second), orientation as a [`Rot2`] and the
//! shapes of [`Shape2`]. Applications use the aliases of this module.

mod rot;
mod shape;

use crate::core::{Dim, MassProperties};
use base::math::Vec2;

pub use crate::core::{BodyKind, PhysicsError};
pub use rot::Rot2;
pub use shape::Shape2;

/// A 2D rigid-body world.
pub type World = crate::core::World<D2>;
/// What a 2D world is built from.
pub type WorldConfig = crate::core::WorldConfig<D2>;
/// The description a 2D body is created from.
pub type BodyDesc = crate::core::BodyDesc<D2>;
/// The state of a 2D body.
pub type Body = crate::core::Body<D2>;
/// Names one body of a 2D world.
pub type BodyHandle = crate::core::BodyHandle<D2>;

/// Standard gravity as a downward acceleration in metres per second squared (right-handed, Y up).
pub const STANDARD_GRAVITY: Vec2 = Vec2::new(0.0, -9.806_65);

/// The 2D dimension marker for [`Dim`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct D2;

impl Dim for D2 {
    type Vec = Vec2;
    type Ang = f32;
    type Rot = Rot2;
    type InvInertia = f32;
    type Shape = Shape2;

    fn apply_inverse_inertia(_rot: Rot2, inv_inertia: f32, torque: f32) -> f32 {
        inv_inertia * torque
    }

    fn advance_rotation(rot: Rot2, angular_velocity: f32, dt: f32) -> Rot2 {
        rot.advanced(angular_velocity * dt)
    }

    fn mass_properties(shape: &Shape2, density: f32) -> Result<MassProperties<Self>, PhysicsError> {
        shape::mass_properties(shape, density)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_gravity_points_down_with_the_standard_magnitude() {
        assert!((STANDARD_GRAVITY.y + 9.806_65).abs() < 1e-6);
        assert!(STANDARD_GRAVITY.x.abs() < f32::EPSILON);
    }
}
