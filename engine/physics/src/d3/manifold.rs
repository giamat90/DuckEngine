//! The 3D contact manifold: one normal and up to four points.

use crate::core::{HashBits, StateHasher};
use base::math::Vec3;

/// The most points a manifold holds: the corners of a clipped face (produced from card 8-4).
pub const MAX_MANIFOLD_POINTS: usize = 4;

/// One contact point.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ContactPoint3 {
    /// World-space position, halfway between the two surfaces (set by the narrowphase of card 8-4).
    pub point: Vec3,
    /// The gap along the normal: negative while the shapes overlap (by how much), positive while they
    /// are apart but within the speculative margin.
    pub separation: f32,
}

/// The contact between two shapes. The normal points from the first body to the second.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Manifold3 {
    /// Unit normal, from the first body to the second.
    pub normal: Vec3,
    /// How many of `points` are in use, at most [`MAX_MANIFOLD_POINTS`].
    pub count: u8,
    /// The points; those from `count` on are unspecified and not hashed.
    pub points: [ContactPoint3; MAX_MANIFOLD_POINTS],
}

impl Manifold3 {
    /// The points in use.
    #[must_use]
    pub fn active(&self) -> &[ContactPoint3] {
        let used = usize::from(self.count).min(MAX_MANIFOLD_POINTS);
        &self.points[..used]
    }

    /// A manifold of `points` (at most [`MAX_MANIFOLD_POINTS`]; any more are dropped).
    #[must_use]
    pub(crate) fn new(normal: Vec3, points: &[ContactPoint3]) -> Self {
        let mut stored = [ContactPoint3::default(); MAX_MANIFOLD_POINTS];
        let used = points.len().min(MAX_MANIFOLD_POINTS);
        stored[..used].copy_from_slice(&points[..used]);
        Self {
            normal,
            count: u8::try_from(used).unwrap_or(0),
            points: stored,
        }
    }

    /// The same contact seen from the other body: the normal turned around.
    #[must_use]
    pub(crate) fn flipped(mut self) -> Self {
        self.normal = -self.normal;
        self
    }

    /// True when the normal and every point in use are finite.
    #[must_use]
    pub(crate) fn is_finite(&self) -> bool {
        let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
        finite(self.normal)
            && self
                .active()
                .iter()
                .all(|p| finite(p.point) && p.separation.is_finite())
    }
}

impl HashBits for Manifold3 {
    fn write_bits(&self, hasher: &mut StateHasher) {
        self.normal.write_bits(hasher);
        hasher.write_u8(self.count);
        for p in self.active() {
            p.point.write_bits(hasher);
            hasher.write_f32(p.separation);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifold(count: u8, separation: f32) -> Manifold3 {
        let mut points = [ContactPoint3::default(); MAX_MANIFOLD_POINTS];
        points[0] = ContactPoint3 {
            point: Vec3::new(1.0, 2.0, 3.0),
            separation,
        };
        points[3] = ContactPoint3 {
            point: Vec3::new(9.0, 9.0, 9.0),
            separation: 7.0,
        };
        Manifold3 {
            normal: Vec3::Y,
            count,
            points,
        }
    }

    fn hash(m: &Manifold3) -> u64 {
        let mut hasher = StateHasher::new();
        m.write_bits(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn only_the_points_in_use_are_visible_and_hashed() {
        let m = manifold(1, 0.5);
        assert_eq!(m.active().len(), 1);
        let mut other = m;
        other.points[3].separation = -1.0;
        assert_eq!(hash(&m), hash(&other), "a point past count is not hashed");
        assert_ne!(
            hash(&m),
            hash(&manifold(4, 0.5)),
            "count and the points it adds are"
        );
    }

    #[test]
    fn the_hash_sees_the_normal_the_count_and_each_point_field() {
        let m = manifold(1, 0.5);
        let mut n = m;
        n.normal = Vec3::X;
        assert_ne!(hash(&m), hash(&n));
        assert_ne!(hash(&m), hash(&manifold(1, 0.25)));
        let mut p = m;
        p.points[0].point.z = 3.5;
        assert_ne!(hash(&m), hash(&p));
        assert_ne!(hash(&manifold(1, 0.5)), hash(&manifold(2, 0.5)));
    }

    #[test]
    fn a_count_above_the_capacity_is_clamped() {
        assert_eq!(manifold(200, 0.5).active().len(), MAX_MANIFOLD_POINTS);
    }

    #[test]
    fn a_manifold_is_finite_when_its_normal_and_points_in_use_are() {
        assert!(manifold(1, 0.5).is_finite());
        let nan = f32::NAN;
        for bad in [
            Vec3::new(nan, 0.0, 1.0),
            Vec3::new(0.0, f32::INFINITY, 1.0),
            Vec3::new(0.0, 1.0, nan),
        ] {
            let mut m = manifold(1, 0.5);
            m.normal = bad;
            assert!(!m.is_finite(), "normal {bad:?}");
            let mut p = manifold(1, 0.5);
            p.points[0].point = bad;
            assert!(!p.is_finite(), "point {bad:?}");
        }
        let mut s = manifold(1, 0.5);
        s.points[0].separation = nan;
        assert!(!s.is_finite());
        let mut beyond = manifold(1, 0.5);
        beyond.points[3].point = Vec3::new(nan, nan, nan);
        assert!(beyond.is_finite(), "a point past count is not looked at");
    }

    #[test]
    fn new_keeps_at_most_four_points_and_flipped_turns_the_normal() {
        let p = ContactPoint3 {
            point: Vec3::X,
            separation: -0.5,
        };
        let five = [p; 5];
        let m = Manifold3::new(Vec3::Y, &five);
        assert_eq!(m.count, 4);
        assert_eq!(Manifold3::new(Vec3::Y, &five[..2]).count, 2);
        assert_eq!(m.flipped().normal, -Vec3::Y);
        assert_eq!(m.flipped().count, 4);
    }
}
