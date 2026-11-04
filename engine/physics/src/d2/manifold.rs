//! The 2D contact manifold: a normal and one or two points, each with its separation, its two
//! anchors and a feature id for warm starting.

use crate::core::{HashBits, Quantity, StateHasher};
use base::math::Vec2;

/// The most points one 2D manifold holds: a face meets a face in two.
pub const MAX_MANIFOLD_POINTS: usize = 2;

/// Names the geometric feature pair that made a contact point (which face of the reference
/// polygon met which vertex of the incident one, and whether the point was cut by a side of the
/// reference face). The same feature pair gives the same id in the next step, so a solver can
/// carry an accumulated impulse over. When the reference face changes (it moves from one body to
/// the other, or to another face) the ids change and the impulse is lost for a step; that is the
/// documented limit of this scheme.
///
/// The id is opaque: only equality is meaningful.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FeatureId(u16);

impl FeatureId {
    /// The id for a feature `kind` and two indices below 16 (each polygon has at most 8 vertices).
    /// Written with arithmetic, not bit operations, because the three fields do not overlap.
    pub(super) fn pack(kind: u8, first: usize, second: usize) -> Self {
        Self(u16::from(kind) * 256 + nibble(first) * 16 + nibble(second))
    }

    /// The raw value, for storing and for tests.
    #[must_use]
    pub const fn raw(self) -> u16 {
        self.0
    }
}

/// An index as a number below 16; a larger index (a polygon has at most 8 vertices, so this does
/// not happen) saturates instead of spilling into the next field.
fn nibble(index: usize) -> u16 {
    u16::try_from(index.min(15)).unwrap_or(15)
}

/// One point of a manifold.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ContactPoint2 {
    /// The point in world coordinates, half way between the two surfaces.
    pub point: Vec2,
    /// The point relative to the centre of the first body (in world orientation).
    pub anchor_a: Vec2,
    /// The point relative to the centre of the second body (in world orientation).
    pub anchor_b: Vec2,
    /// The signed distance between the surfaces along the normal: negative when they overlap
    /// (that is the penetration depth), positive for a speculative contact that does not touch yet.
    pub separation: f32,
    /// The feature pair that made the point (see [`FeatureId`]).
    pub id: FeatureId,
}

impl ContactPoint2 {
    fn is_finite(&self) -> bool {
        self.point.is_finite()
            && self.anchor_a.is_finite()
            && self.anchor_b.is_finite()
            && self.separation.is_finite()
    }

    fn flipped(self) -> Self {
        Self {
            anchor_a: self.anchor_b,
            anchor_b: self.anchor_a,
            ..self
        }
    }
}

/// The contact of two shapes: a unit normal pointing from the first shape to the second and one or
/// two points. Unused point slots hold the default value, so equal manifolds compare and hash
/// equal.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Manifold2 {
    /// The unit contact normal, from the first shape to the second.
    pub normal: Vec2,
    count: usize,
    points: [ContactPoint2; MAX_MANIFOLD_POINTS],
}

impl Manifold2 {
    /// A manifold from `normal` and its points; at most [`MAX_MANIFOLD_POINTS`] are kept.
    pub(super) fn new(normal: Vec2, points: &[ContactPoint2]) -> Self {
        let mut stored = [ContactPoint2::default(); MAX_MANIFOLD_POINTS];
        let count = points.len().min(MAX_MANIFOLD_POINTS);
        stored[..count].copy_from_slice(&points[..count]);
        Self {
            normal,
            count,
            points: stored,
        }
    }

    /// The points of the manifold, one or two.
    #[must_use]
    pub fn points(&self) -> &[ContactPoint2] {
        &self.points[..self.count]
    }

    /// The same contact seen from the other shape: the normal is negated and the anchors swap.
    #[must_use]
    pub fn flipped(self) -> Self {
        Self {
            normal: Vec2::new(-self.normal.x, -self.normal.y),
            count: self.count,
            points: self.points.map(ContactPoint2::flipped),
        }
    }

    /// True when the normal and every point hold finite numbers only.
    pub(super) fn is_finite(&self) -> bool {
        self.normal.is_finite() && self.points().iter().all(ContactPoint2::is_finite)
    }
}

impl HashBits for Manifold2 {
    fn write_bits(&self, hasher: &mut StateHasher) {
        self.normal.write_bits(hasher);
        hasher.write_u8(u8::try_from(self.count).unwrap_or(u8::MAX));
        for point in &self.points {
            point.point.write_bits(hasher);
            point.anchor_a.write_bits(hasher);
            point.anchor_b.write_bits(hasher);
            hasher.write_f32(point.separation);
            hasher.write_u32(u32::from(point.id.raw()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: f32, id: u16) -> ContactPoint2 {
        ContactPoint2 {
            point: Vec2::new(x, 1.0),
            anchor_a: Vec2::new(x, 2.0),
            anchor_b: Vec2::new(x, 3.0),
            separation: -0.25,
            id: FeatureId(id),
        }
    }

    fn two() -> Manifold2 {
        Manifold2::new(Vec2::new(0.0, 1.0), &[point(1.0, 5), point(2.0, 6)])
    }

    #[test]
    fn a_feature_id_is_the_kind_and_two_indices_without_overlap() {
        assert_eq!(FeatureId::pack(0, 0, 0).raw(), 0);
        assert_eq!(FeatureId::pack(1, 0, 0).raw(), 256);
        assert_eq!(FeatureId::pack(0, 1, 0).raw(), 16);
        assert_eq!(FeatureId::pack(0, 0, 1).raw(), 1);
        assert_eq!(FeatureId::pack(3, 7, 5).raw(), 3 * 256 + 7 * 16 + 5);
        // An index that does not fit saturates instead of wrapping into another feature.
        assert_eq!(FeatureId::pack(0, 70_000, 0).raw(), 15 * 16);
        assert_eq!(FeatureId::pack(0, 0, 70_000).raw(), 15);
        assert_eq!(FeatureId::pack(0, 15, 15).raw(), 255);
    }

    #[test]
    fn a_manifold_holds_one_or_two_points_and_ignores_the_rest() {
        let one = Manifold2::new(Vec2::X, &[point(1.0, 1)]);
        assert_eq!(one.points().len(), 1);
        assert_eq!(two().points().len(), 2);
        let many = Manifold2::new(Vec2::X, &[point(1.0, 1), point(2.0, 2), point(3.0, 3)]);
        assert_eq!(many.points().len(), MAX_MANIFOLD_POINTS);
        assert_eq!(many.points()[1].id, FeatureId(2));
        assert!(Manifold2::default().points().is_empty());
    }

    #[test]
    fn flipping_negates_the_normal_and_swaps_the_anchors_only() {
        let m = two();
        let f = m.flipped();
        assert_eq!(f.normal, Vec2::new(-0.0, -1.0));
        assert_eq!(f.points().len(), 2);
        for (a, b) in m.points().iter().zip(f.points()) {
            assert_eq!(
                (a.point, a.anchor_a, a.anchor_b),
                (b.point, b.anchor_b, b.anchor_a)
            );
            assert_eq!((a.separation, a.id), (b.separation, b.id));
        }
        assert_eq!(f.flipped(), m);
        let slanted = Manifold2::new(Vec2::new(0.6, -0.8), &[point(1.0, 5)]).flipped();
        assert_eq!(slanted.normal, Vec2::new(-0.6, 0.8));
    }

    #[test]
    fn finiteness_covers_the_normal_and_every_field_of_every_point() {
        assert!(two().is_finite());
        assert!(Manifold2::default().is_finite());
        let nan = f32::NAN;
        let mut cases = vec![];
        let mut m = two();
        m.normal = Vec2::new(nan, 0.0);
        cases.push(m);
        for field in 0..4 {
            let mut m = two();
            let p = &mut m.points[1];
            match field {
                0 => p.point.y = f32::INFINITY,
                1 => p.anchor_a.x = nan,
                2 => p.anchor_b.y = nan,
                _ => p.separation = nan,
            }
            cases.push(m);
        }
        for m in cases {
            assert!(!m.is_finite(), "{m:?}");
        }
        // A slot past the count is not looked at.
        let mut m = Manifold2::new(Vec2::X, &[point(1.0, 1)]);
        m.points[1].separation = nan;
        assert!(m.is_finite());
    }

    #[test]
    fn the_hash_covers_every_field() {
        let hash = |m: &Manifold2| {
            let mut h = StateHasher::new();
            m.write_bits(&mut h);
            h.finish()
        };
        let base = two();
        let mut variants = vec![];
        let mut m = base;
        m.normal.x = 1.0;
        variants.push(m);
        variants.push(Manifold2::new(base.normal, &base.points()[..1]));
        for field in 0..5 {
            let mut m = base;
            let p = &mut m.points[1];
            match field {
                0 => p.point.x += 1.0,
                1 => p.anchor_a.x += 1.0,
                2 => p.anchor_b.x += 1.0,
                3 => p.separation += 1.0,
                _ => p.id = FeatureId(99),
            }
            variants.push(m);
        }
        for v in variants {
            assert_ne!(hash(&v), hash(&base), "{v:?}");
        }
        assert_eq!(hash(&two()), hash(&base));
    }
}
