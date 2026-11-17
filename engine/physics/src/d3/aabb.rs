//! The 3D axis-aligned bounding box.

use crate::core::{Bounds, HashBits, StateHasher};
use base::math::Vec3;

/// An axis-aligned box in world space, from its smallest to its largest corner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Aabb3 {
    /// The corner with the smallest coordinates.
    pub min: Vec3,
    /// The corner with the largest coordinates.
    pub max: Vec3,
}

impl Aabb3 {
    /// A box from its two corners. Not checked: [`Bounds::is_valid`] says whether it is usable.
    #[must_use]
    pub const fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }
}

impl HashBits for Aabb3 {
    fn write_bits(&self, hasher: &mut StateHasher) {
        self.min.write_bits(hasher);
        self.max.write_bits(hasher);
    }
}

impl Bounds for Aabb3 {
    fn overlaps(&self, other: &Self) -> bool {
        self.min.x <= other.max.x
            && other.min.x <= self.max.x
            && self.min.y <= other.max.y
            && other.min.y <= self.max.y
            && self.min.z <= other.max.z
            && other.min.z <= self.max.z
    }

    fn union(&self, other: &Self) -> Self {
        Self::new(self.min.min(other.min), self.max.max(other.max))
    }

    fn contains(&self, inner: &Self) -> bool {
        self.min.x <= inner.min.x
            && self.min.y <= inner.min.y
            && self.min.z <= inner.min.z
            && inner.max.x <= self.max.x
            && inner.max.y <= self.max.y
            && inner.max.z <= self.max.z
    }

    /// The surface area, the cost the broadphase tree minimises (the perimeter in 2D).
    fn measure(&self) -> f32 {
        let d = self.max - self.min;
        2.0 * (d.x * d.y + d.y * d.z + d.z * d.x)
    }

    fn fattened(&self, margin: f32) -> Self {
        let m = Vec3::new(margin, margin, margin);
        Self::new(self.min - m, self.max + m)
    }

    fn is_valid(&self) -> bool {
        let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
        finite(self.min)
            && finite(self.max)
            && self.min.x <= self.max.x
            && self.min.y <= self.max.y
            && self.min.z <= self.max.z
    }
}
