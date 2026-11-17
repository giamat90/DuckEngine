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
    /// How far the shapes overlap along the normal; negative while they are apart but within the
    /// speculative margin.
    pub depth: f32,
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
}

impl HashBits for Manifold3 {
    fn write_bits(&self, hasher: &mut StateHasher) {
        self.normal.write_bits(hasher);
        hasher.write_u8(self.count);
        for p in self.active() {
            p.point.write_bits(hasher);
            hasher.write_f32(p.depth);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifold(count: u8, depth: f32) -> Manifold3 {
        let mut points = [ContactPoint3::default(); MAX_MANIFOLD_POINTS];
        points[0] = ContactPoint3 {
            point: Vec3::new(1.0, 2.0, 3.0),
            depth,
        };
        points[3] = ContactPoint3 {
            point: Vec3::new(9.0, 9.0, 9.0),
            depth: 7.0,
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
        other.points[3].depth = -1.0;
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
}
