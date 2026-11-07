//! The 2D axis-aligned bounding box.

use crate::core::{Bounds, HashBits, StateHasher};
use base::math::Vec2;

/// A rectangle with sides parallel to the axes, from `min` to `max`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Aabb2 {
    /// The corner with the smallest coordinates.
    pub min: Vec2,
    /// The corner with the largest coordinates.
    pub max: Vec2,
}

impl Aabb2 {
    /// The box from `min` to `max` (not checked: see [`Bounds::is_valid`]).
    #[must_use]
    pub const fn new(min: Vec2, max: Vec2) -> Self {
        Self { min, max }
    }
}

impl HashBits for Aabb2 {
    fn write_bits(&self, hasher: &mut StateHasher) {
        self.min.write_bits(hasher);
        self.max.write_bits(hasher);
    }
}

impl Bounds for Aabb2 {
    fn overlaps(&self, other: &Self) -> bool {
        self.min.x <= other.max.x
            && other.min.x <= self.max.x
            && self.min.y <= other.max.y
            && other.min.y <= self.max.y
    }

    fn union(&self, other: &Self) -> Self {
        Self::new(
            Vec2::new(self.min.x.min(other.min.x), self.min.y.min(other.min.y)),
            Vec2::new(self.max.x.max(other.max.x), self.max.y.max(other.max.y)),
        )
    }

    fn contains(&self, inner: &Self) -> bool {
        self.min.x <= inner.min.x
            && self.min.y <= inner.min.y
            && inner.max.x <= self.max.x
            && inner.max.y <= self.max.y
    }

    fn measure(&self) -> f32 {
        2.0 * ((self.max.x - self.min.x) + (self.max.y - self.min.y))
    }

    fn fattened(&self, margin: f32) -> Self {
        Self::new(
            Vec2::new(self.min.x - margin, self.min.y - margin),
            Vec2::new(self.max.x + margin, self.max.y + margin),
        )
    }

    fn is_valid(&self) -> bool {
        self.min.x.is_finite()
            && self.min.y.is_finite()
            && self.max.x.is_finite()
            && self.max.y.is_finite()
            && self.min.x <= self.max.x
            && self.min.y <= self.max.y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(x0: f32, y0: f32, x1: f32, y1: f32) -> Aabb2 {
        Aabb2::new(Vec2::new(x0, y0), Vec2::new(x1, y1))
    }

    #[test]
    fn boxes_that_share_area_overlap() {
        assert!(b(0.0, 0.0, 2.0, 2.0).overlaps(&b(1.0, 1.0, 3.0, 3.0)));
        assert!(b(1.0, 1.0, 3.0, 3.0).overlaps(&b(0.0, 0.0, 2.0, 2.0)));
        assert!(b(0.0, 0.0, 4.0, 4.0).overlaps(&b(1.0, 1.0, 2.0, 2.0)));
    }

    #[test]
    fn boxes_that_only_touch_overlap() {
        assert!(b(0.0, 0.0, 1.0, 1.0).overlaps(&b(1.0, 0.0, 2.0, 1.0)));
        assert!(b(0.0, 0.0, 1.0, 1.0).overlaps(&b(0.0, 1.0, 1.0, 2.0)));
        assert!(b(0.0, 0.0, 1.0, 1.0).overlaps(&b(1.0, 1.0, 2.0, 2.0)));
    }

    #[test]
    fn boxes_with_a_gap_on_either_axis_do_not_overlap() {
        let a = b(0.0, 0.0, 1.0, 1.0);
        assert!(!a.overlaps(&b(1.5, 0.0, 2.0, 1.0)));
        assert!(!a.overlaps(&b(0.0, 1.5, 1.0, 2.0)));
        assert!(!a.overlaps(&b(-2.0, 0.0, -0.5, 1.0)));
        assert!(!a.overlaps(&b(0.0, -2.0, 1.0, -0.5)));
    }

    #[test]
    fn the_union_holds_both_boxes() {
        let u = b(0.0, 1.0, 2.0, 3.0).union(&b(-1.0, 2.0, 1.0, 5.0));
        assert_eq!(u, b(-1.0, 1.0, 2.0, 5.0));
    }

    #[test]
    fn a_box_contains_itself_and_what_is_inside_but_not_what_sticks_out() {
        let outer = b(0.0, 0.0, 4.0, 4.0);
        assert!(outer.contains(&outer));
        assert!(outer.contains(&b(1.0, 1.0, 2.0, 2.0)));
        assert!(!outer.contains(&b(1.0, 1.0, 5.0, 2.0)));
        assert!(!outer.contains(&b(-1.0, 1.0, 2.0, 2.0)));
        assert!(!outer.contains(&b(1.0, 1.0, 2.0, 5.0)));
        assert!(!outer.contains(&b(1.0, -1.0, 2.0, 2.0)));
    }

    #[test]
    fn the_measure_is_the_perimeter() {
        assert!((b(0.0, 0.0, 3.0, 2.0).measure() - 10.0).abs() < f32::EPSILON);
        assert!(b(1.0, 1.0, 1.0, 1.0).measure().abs() < f32::EPSILON);
    }

    #[test]
    fn fattening_grows_every_side() {
        assert_eq!(b(0.0, 1.0, 2.0, 3.0).fattened(0.5), b(-0.5, 0.5, 2.5, 3.5));
    }

    #[test]
    fn a_box_is_valid_when_finite_and_ordered() {
        assert!(b(0.0, 0.0, 1.0, 1.0).is_valid());
        assert!(b(1.0, 1.0, 1.0, 1.0).is_valid());
        assert!(!b(2.0, 0.0, 1.0, 1.0).is_valid());
        assert!(!b(0.0, 2.0, 1.0, 1.0).is_valid());
        assert!(!b(f32::NAN, 0.0, 1.0, 1.0).is_valid());
        assert!(!b(0.0, 0.0, 1.0, f32::INFINITY).is_valid());
        assert!(!b(0.0, f32::NEG_INFINITY, 1.0, 1.0).is_valid());
        assert!(!b(0.0, 0.0, f32::NAN, 1.0).is_valid());
    }

    #[test]
    fn the_hash_covers_every_coordinate() {
        let hash = |a: Aabb2| {
            let mut h = StateHasher::new();
            a.write_bits(&mut h);
            h.finish()
        };
        let base = b(0.0, 1.0, 2.0, 3.0);
        for other in [
            b(9.0, 1.0, 2.0, 3.0),
            b(0.0, 9.0, 2.0, 3.0),
            b(0.0, 1.0, 9.0, 3.0),
            b(0.0, 1.0, 2.0, 9.0),
        ] {
            assert_ne!(hash(base), hash(other));
        }
    }
}
