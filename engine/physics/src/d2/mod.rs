//! The 2D instance of the core: the [`D2`] marker, position and velocity as `Vec2`, angular
//! quantities as `f32` (counter-clockwise, radians per second), orientation as a [`Rot2`] and the
//! shapes of [`Shape2`]. Applications use the aliases of this module.

mod aabb;
mod manifold;
mod narrowphase;
mod polygon;
mod rot;
mod shape;

use crate::core::{Dim, MassProperties, Placed};
use base::math::Vec2;

pub use crate::core::{
    BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, MAX_PAIRS, PhysicsError,
};
pub use aabb::Aabb2;
pub use manifold::{ContactPoint2, FeatureId, MAX_MANIFOLD_POINTS, Manifold2};
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
/// Two bodies of a 2D world whose broadphase boxes overlap.
pub type Pair = crate::core::Pair<D2>;
/// The pairs of a 2D world, in canonical order.
pub type PairList = crate::core::PairList<D2>;
/// A 2D shape put in the world (see [`Dim::collide`]).
pub type Placed2 = Placed<D2>;
/// The manifold of one pair of a 2D world.
pub type ContactManifold = crate::core::ContactManifold<D2>;
/// The manifolds of a 2D world, in the order of the pair list.
pub type ManifoldList = crate::core::ManifoldList<D2>;
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
    type Bounds = Aabb2;
    type Manifold = Manifold2;

    fn apply_inverse_inertia(_rot: Rot2, inv_inertia: f32, torque: f32) -> f32 {
        inv_inertia * torque
    }

    fn advance_rotation(rot: Rot2, angular_velocity: f32, dt: f32) -> Rot2 {
        rot.advanced(angular_velocity * dt)
    }

    fn shape_bounds(shape: &Shape2, position: Vec2, rot: Rot2) -> Aabb2 {
        shape::bounds(shape, position, rot)
    }

    fn collide(a: &Placed<Self>, b: &Placed<Self>, margin: f32) -> Option<Manifold2> {
        narrowphase::collide(a, b, margin)
    }

    fn mass_properties(shape: &Shape2, density: f32) -> Result<MassProperties<Self>, PhysicsError> {
        shape::mass_properties(shape, density)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Bounds;

    fn bounds_of(shape: Shape2, x: f32, y: f32, angle: f32) -> Aabb2 {
        D2::shape_bounds(&shape, Vec2::new(x, y), Rot2::from_angle(angle))
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn a_circle_is_bounded_by_its_radius_at_any_angle() {
        for angle in [0.0, 0.7, 2.0] {
            let b = bounds_of(Shape2::Circle { radius: 2.0 }, 1.0, -3.0, angle);
            assert!(near(b.min.x, -1.0) && near(b.max.x, 3.0), "{b:?}");
            assert!(near(b.min.y, -5.0) && near(b.max.y, -1.0), "{b:?}");
        }
    }

    #[test]
    fn an_unrotated_box_is_bounded_by_its_half_extents() {
        let shape = Shape2::Box {
            half_extents: Vec2::new(2.0, 1.0),
        };
        let b = bounds_of(shape, 10.0, 5.0, 0.0);
        assert_eq!(b, Aabb2::new(Vec2::new(8.0, 4.0), Vec2::new(12.0, 6.0)));
    }

    #[test]
    fn a_box_turned_by_a_quarter_turn_swaps_its_extents() {
        let shape = Shape2::Box {
            half_extents: Vec2::new(2.0, 0.5),
        };
        let b = bounds_of(shape, 0.0, 0.0, std::f32::consts::FRAC_PI_2);
        assert!(near(b.max.x, 0.5) && near(b.max.y, 2.0), "{b:?}");
        assert!(near(b.min.x, -0.5) && near(b.min.y, -2.0), "{b:?}");
    }

    #[test]
    fn a_box_turned_by_an_eighth_of_a_turn_grows_by_the_sum_of_its_extents() {
        let shape = Shape2::Box {
            half_extents: Vec2::new(2.0, 0.5),
        };
        let b = bounds_of(shape, 0.0, 0.0, std::f32::consts::FRAC_PI_4);
        let expected = 2.5 * std::f32::consts::FRAC_1_SQRT_2;
        assert!(near(b.max.x, expected) && near(b.max.y, expected), "{b:?}");
        assert!(
            near(b.min.x, -expected) && near(b.min.y, -expected),
            "{b:?}"
        );
    }

    #[test]
    fn a_position_that_is_not_finite_or_overflows_gives_bounds_that_are_not_valid() {
        let circle = Shape2::Circle { radius: 1.0 };
        assert!(!bounds_of(circle, f32::NAN, 0.0, 0.0).is_valid());
        assert!(!bounds_of(circle, f32::INFINITY, 0.0, 0.0).is_valid());
        let huge = Shape2::Circle { radius: 3.0e38 };
        assert!(!bounds_of(huge, f32::MAX, 0.0, 0.0).is_valid());
        assert!(!bounds_of(huge, -f32::MAX, 0.0, 0.0).is_valid());
        assert!(bounds_of(circle, 0.0, 0.0, 0.0).is_valid());
    }

    #[test]
    fn standard_gravity_points_down_with_the_standard_magnitude() {
        assert!((STANDARD_GRAVITY.y + 9.806_65).abs() < 1e-6);
        assert!(STANDARD_GRAVITY.x.abs() < f32::EPSILON);
    }
}
