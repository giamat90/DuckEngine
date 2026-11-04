//! The 2D narrowphase: which routine handles which pair of shapes, and the conversion of a result
//! in the working frame (origin at the centre of one body) to a [`Manifold2`] in world terms.
//!
//! | first / second | circle | box |
//! |---|---|---|
//! | circle | closest points of two discs | the box's routine with the roles swapped, result flipped |
//! | box | circle against polygon (face and vertex regions) | separating axis, reference face, clipping |
//!
//! Nothing here calls `sin`, `cos` or `atan2`: a rotation is its `(cos, sin)` pair. Only `+ - * /`
//! and `sqrt` are used, in a fixed order, so the result is the same on every IEEE-754 platform.
//! Any input that is not valid (a shape out of range, a value that is not finite) and any result
//! that is not finite (an overflow) gives `None`, never a manifold with a NaN in it.

use super::manifold::{ContactPoint2, FeatureId, Manifold2};
use super::polygon::{
    CircleQuery, Local, LocalPoint, MIN_LENGTH_SQUARED, Poly, collide_polygons, separated,
};
use super::{D2, Shape2};
use crate::core::{Placed, Quantity};
use base::math::Vec2;

/// The unit normal used when two circles have their centres in the same place (or so close that no
/// direction can be taken): the x axis, the same on every platform.
const COINCIDENT_NORMAL: Vec2 = Vec2::X;

/// Two circles of radii `ra` and `rb` whose centres are `offset` apart (from the first to the
/// second).
fn circles(ra: f32, rb: f32, offset: Vec2, margin: f32) -> Option<Local> {
    let length_squared = offset.length_squared();
    let length = length_squared.sqrt();
    let separation = length - ra - rb;
    if separated(separation, margin) {
        return None;
    }
    let normal = if length_squared > MIN_LENGTH_SQUARED {
        Vec2::new(offset.x / length, offset.y / length)
    } else {
        COINCIDENT_NORMAL
    };
    let on_first = normal * ra;
    let on_second = offset - normal * rb;
    Some(local_point(
        normal,
        (on_first + on_second) * 0.5,
        separation,
    ))
}

/// A one-point result in the working frame.
fn local_point(normal: Vec2, position: Vec2, separation: f32) -> Local {
    Local::new(
        normal,
        &[LocalPoint {
            position,
            separation,
            id: FeatureId::default(),
        }],
    )
}

/// A result of the working frame as a manifold. `origin` is the centre of the body the frame is
/// centred on, `offset` the vector from it to the other body, and `swapped` tells that this body
/// is the second one of the pair (the result is then turned around).
fn to_manifold(local: &Local, origin: Vec2, offset: Vec2, swapped: bool) -> Manifold2 {
    let mut points = [ContactPoint2::default(); 2];
    for (slot, point) in points.iter_mut().zip(local.points()) {
        *slot = ContactPoint2 {
            point: origin + point.position,
            anchor_a: point.position,
            anchor_b: point.position - offset,
            separation: point.separation,
            id: point.id,
        };
    }
    let manifold = Manifold2::new(local.normal, &points[..local.points().len()]);
    if swapped {
        manifold.flipped()
    } else {
        manifold
    }
}

/// True when the shape's sizes are valid and the placement is finite.
fn is_valid(placed: &Placed<D2>) -> bool {
    super::shape::is_valid(&placed.shape)
        && placed.position.is_finite()
        && Quantity::is_finite(&placed.rotation)
}

fn box_of(placed: &Placed<D2>, half: Vec2, origin: Vec2) -> Poly {
    Poly::from_box(half, placed.rotation, origin)
}

/// The result for a polygon and a circle in the frame of the polygon's body.
fn polygon_and_circle(polygon: &Poly, radius: f32, centre: Vec2, margin: f32) -> Option<Local> {
    CircleQuery {
        polygon,
        radius,
        centre,
        margin,
    }
    .collide()
}

/// The manifold of two placed 2D shapes, normal from `a` to `b` (see [`crate::core::Dim::collide`]).
pub(super) fn collide(a: &Placed<D2>, b: &Placed<D2>, margin: f32) -> Option<Manifold2> {
    if !(is_valid(a) && is_valid(b) && margin >= 0.0) {
        return None;
    }
    let offset = b.position - a.position;
    let manifold = match (a.shape, b.shape) {
        (Shape2::Circle { radius: ra }, Shape2::Circle { radius: rb }) => {
            circles(ra, rb, offset, margin).map(|l| to_manifold(&l, a.position, offset, false))
        }
        (Shape2::Box { half_extents }, Shape2::Circle { radius }) => {
            let polygon = box_of(a, half_extents, Vec2::ZERO);
            polygon_and_circle(&polygon, radius, offset, margin)
                .map(|l| to_manifold(&l, a.position, offset, false))
        }
        (Shape2::Circle { radius }, Shape2::Box { half_extents }) => {
            let polygon = box_of(b, half_extents, Vec2::ZERO);
            let back = Vec2::new(-offset.x, -offset.y);
            polygon_and_circle(&polygon, radius, back, margin)
                .map(|l| to_manifold(&l, b.position, back, true))
        }
        (Shape2::Box { half_extents: ha }, Shape2::Box { half_extents: hb }) => {
            let (first, second) = (box_of(a, ha, Vec2::ZERO), box_of(b, hb, offset));
            collide_polygons(&first, &second, margin)
                .map(|l| to_manifold(&l, a.position, offset, false))
        }
    }?;
    manifold.is_finite().then_some(manifold)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::d2::Rot2;
    use crate::test_support::Must;

    fn circle(radius: f32, x: f32, y: f32) -> Placed<D2> {
        Placed::new(Shape2::Circle { radius }, Vec2::new(x, y), Rot2::IDENTITY)
    }

    fn boxed(hx: f32, hy: f32, x: f32, y: f32) -> Placed<D2> {
        Placed::new(
            Shape2::Box {
                half_extents: Vec2::new(hx, hy),
            },
            Vec2::new(x, y),
            Rot2::IDENTITY,
        )
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn centres_closer_than_the_smallest_normal_distance_use_the_fixed_normal() {
        // The squared distance is exactly f32::MIN_POSITIVE (2^-63 squared): the last distance for
        // which no direction is taken; one step further the real direction is used.
        let tiny = f32::from_bits(0x2000_0000);
        assert_eq!((tiny * tiny).to_bits(), f32::MIN_POSITIVE.to_bits());
        let at_limit = collide(&circle(1.0, 0.0, 0.0), &circle(1.0, -tiny, 0.0), 0.0).must("limit");
        assert_eq!(at_limit.normal, Vec2::X);
        let beyond = f32::from_bits(0x2000_0001);
        let past = collide(&circle(1.0, 0.0, 0.0), &circle(1.0, -beyond, 0.0), 0.0).must("past");
        assert_eq!(past.normal, Vec2::new(-1.0, 0.0));
    }

    #[test]
    fn two_overlapping_circles_touch_half_way_along_the_line_of_centres() {
        let m = collide(&circle(1.0, 0.0, 0.0), &circle(1.0, 1.5, 0.0), 0.0).must("overlap");
        assert_eq!(m.normal, Vec2::new(1.0, 0.0));
        let [p] = m.points() else { panic!("one point") };
        assert!(near(p.separation, -0.5));
        assert!(near(p.point.x, 0.75) && near(p.point.y, 0.0));
        assert_eq!(p.anchor_a, Vec2::new(0.75, 0.0));
        assert_eq!(p.anchor_b, Vec2::new(-0.75, 0.0));
        assert_eq!(p.id, FeatureId::default());
    }

    #[test]
    fn the_circle_normal_follows_the_direction_between_the_centres() {
        let m = collide(&circle(1.0, 1.0, 1.0), &circle(1.0, 1.0, 2.5), 0.0).must("overlap");
        assert_eq!(m.normal, Vec2::new(0.0, 1.0));
        let m = collide(&circle(1.0, 0.0, 0.0), &circle(1.0, 1.2, 1.6), 0.0).must("overlap");
        assert!(near(m.normal.x, 0.6) && near(m.normal.y, 0.8));
        assert!(near(m.points()[0].separation, -0.0));
    }

    #[test]
    fn circles_further_apart_than_the_margin_do_not_touch() {
        let far = circle(1.0, 2.5, 0.0);
        let zero = circle(1.0, 0.0, 0.0);
        assert!(collide(&zero, &far, 0.0).is_none());
        assert!(collide(&zero, &far, 0.49).is_none());
        let m = collide(&zero, &far, 0.51).must("speculative");
        assert!(near(m.points()[0].separation, 0.5));
        // Exactly touching.
        let touching = collide(&zero, &circle(1.0, 2.0, 0.0), 0.0).must("touching");
        assert_eq!(touching.points()[0].separation.to_bits(), 0.0_f32.to_bits());
    }

    #[test]
    fn circles_with_the_same_centre_get_the_x_axis_and_the_full_depth() {
        let m = collide(&circle(1.0, 3.0, 4.0), &circle(0.5, 3.0, 4.0), 0.0).must("overlap");
        assert_eq!(m.normal, Vec2::new(1.0, 0.0));
        assert!(near(m.points()[0].separation, -1.5));
        assert!(m.is_finite());
        // Centres so close that no direction exists count as the same place.
        let m = collide(&circle(1.0, 0.0, 0.0), &circle(1.0, 1.0e-30, 0.0), 0.0).must("overlap");
        assert_eq!(m.normal, Vec2::new(1.0, 0.0));
    }

    #[test]
    fn a_box_and_a_circle_swap_roles_exactly() {
        let b = boxed(1.0, 1.0, 0.0, 0.0);
        let c = circle(0.5, 0.3, 1.45);
        let forward = collide(&b, &c, 0.0).must("overlap");
        let backward = collide(&c, &b, 0.0).must("overlap");
        assert_eq!(backward, forward.flipped());
        assert_eq!(forward.normal, Vec2::new(0.0, 1.0));
        let p = forward.points()[0];
        // Anchors are relative to each body's own centre.
        assert!(near(p.anchor_a.y, 0.975) && near(p.anchor_b.y, 0.975 - 1.45));
        assert!(near(p.point.y, 0.975));
    }

    #[test]
    fn two_boxes_collide_through_the_polygon_routine_with_anchors_to_each_centre() {
        let a = boxed(1.0, 1.0, 5.0, 5.0);
        let b = boxed(1.0, 1.0, 5.0, 6.9);
        let m = collide(&a, &b, 0.0).must("overlap");
        assert_eq!(m.normal, Vec2::new(0.0, 1.0));
        assert_eq!(m.points().len(), 2);
        for p in m.points() {
            assert!(near(p.separation, -0.1));
            assert!(near(p.point.y, 5.95));
            assert!(near(p.anchor_a.y, p.point.y - 5.0));
            assert!(near(p.anchor_a.x, p.point.x - 5.0));
            assert!(near(p.anchor_b.y, p.point.y - 6.9));
        }
    }

    #[test]
    fn a_translation_of_both_bodies_changes_no_anchor_bit() {
        let a = boxed(1.0, 0.5, 0.25, 0.5);
        let b = boxed(0.75, 0.5, 1.5, 0.875);
        let shift = |p: &Placed<D2>| Placed {
            position: p.position + Vec2::new(1024.0, -2048.0),
            ..*p
        };
        let m1 = collide(&a, &b, 0.1).must("contact");
        let m2 = collide(&shift(&a), &shift(&b), 0.1).must("contact");
        assert_eq!(m1.normal, m2.normal);
        assert_eq!(m1.points().len(), m2.points().len());
        for (p, q) in m1.points().iter().zip(m2.points()) {
            assert_eq!(p.anchor_a, q.anchor_a);
            assert_eq!(p.anchor_b, q.anchor_b);
            assert_eq!(p.separation.to_bits(), q.separation.to_bits());
            assert_eq!(p.id, q.id);
        }
    }

    #[test]
    fn invalid_shapes_non_finite_placements_and_a_bad_margin_give_no_manifold() {
        let good = boxed(1.0, 1.0, 0.0, 0.0);
        let bad_shapes = [
            circle(0.0, 0.0, 0.0),
            circle(-1.0, 0.0, 0.0),
            circle(f32::NAN, 0.0, 0.0),
            boxed(0.0, 1.0, 0.0, 0.0),
            boxed(1.0, -1.0, 0.0, 0.0),
            boxed(f32::INFINITY, 1.0, 0.0, 0.0),
        ];
        for bad in bad_shapes {
            assert!(
                collide(&good, &bad, 0.1).is_none(),
                "{:?} at {:?}",
                bad.shape,
                bad.position
            );
            assert!(
                collide(&bad, &good, 0.1).is_none(),
                "{:?} at {:?}",
                bad.shape,
                bad.position
            );
        }
        let unlucky = [
            circle(1.0, f32::NAN, 0.0),
            circle(1.0, 0.0, f32::INFINITY),
            boxed(1.0, 1.0, f32::NEG_INFINITY, 0.0),
        ];
        for bad in unlucky {
            assert!(
                collide(&good, &bad, 0.1).is_none(),
                "{:?} at {:?}",
                bad.shape,
                bad.position
            );
        }
        let mut no_rotation = good;
        no_rotation.rotation = Rot2::from_angle(f32::NAN);
        assert!(collide(&good, &no_rotation, 0.1).is_none());
        for margin in [-0.1, f32::NAN] {
            assert!(collide(&good, &good, margin).is_none(), "{margin}");
        }
        assert!(collide(&good, &good, 0.0).is_some());
    }

    #[test]
    fn a_result_that_overflows_gives_no_manifold() {
        // The offset overflows: the squared length is infinite.
        let a = circle(1.0, -3.0e38, 0.0);
        let b = circle(1.0, 3.0e38, 0.0);
        assert!(collide(&a, &b, 1.0e38).is_none());
        // Huge circles that really overlap: the points are finite, or there is no manifold.
        let big_a = circle(3.0e38, 0.0, 0.0);
        let big_b = circle(3.0e38, 1.0, 0.0);
        if let Some(m) = collide(&big_a, &big_b, 0.0) {
            assert!(m.is_finite());
        }
        let wide_a = boxed(3.0e38, 3.0e38, 0.0, 0.0);
        let wide_b = boxed(3.0e38, 3.0e38, 1.0, 0.0);
        if let Some(m) = collide(&wide_a, &wide_b, 0.0) {
            assert!(m.is_finite());
        }
    }
}
