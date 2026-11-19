//! The warm start: the impulses of the last step's contacts, kept by pair and matched to the points
//! of the next step by position, because a 3D contact point has no feature id (card 8-4). The
//! manifold list is in pair order, so the old entries are found with a cursor that only moves
//! forward: no search, no hash map, the same result on every run.

use crate::core::{BodyHandle, HashBits, Quantity, StateHasher};
use crate::d3::{D3, MAX_MANIFOLD_POINTS};
use base::math::Vec3;

/// How far, in metres, a point may have moved since the last step and still be the same contact.
/// A resting body moves far less than this in a step; a point that moved more starts from zero.
pub(super) const WARM_MATCH_DISTANCE: f32 = 0.02;

/// The accumulated impulses of one contact point.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct CachedPoint {
    /// Where the point was, in world space.
    pub at: Vec3,
    /// Along the normal, zero or more.
    pub normal: f32,
    /// Across the normal, in world axes.
    pub tangent: Vec3,
}

impl CachedPoint {
    fn is_finite(&self) -> bool {
        self.at.is_finite() && self.normal.is_finite() && self.tangent.is_finite()
    }
}

/// The impulses of one manifold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CachedManifold {
    pub first: BodyHandle<D3>,
    pub second: BodyHandle<D3>,
    count: usize,
    points: [CachedPoint; MAX_MANIFOLD_POINTS],
}

impl CachedManifold {
    pub(super) fn new(
        first: BodyHandle<D3>,
        second: BodyHandle<D3>,
        points: &[CachedPoint],
    ) -> Self {
        let mut stored = [CachedPoint::default(); MAX_MANIFOLD_POINTS];
        let count = points.len().min(MAX_MANIFOLD_POINTS);
        stored
            .iter_mut()
            .zip(points)
            .for_each(|(slot, point)| *slot = *point);
        Self {
            first,
            second,
            count,
            points: stored,
        }
    }

    fn points(&self) -> &[CachedPoint] {
        self.points.get(..self.count).unwrap_or(&[])
    }

    /// The kept point nearest to `at` within [`WARM_MATCH_DISTANCE`] that `taken` (one bit per kept
    /// point) does not mark yet; marks it. The first of two equally near points wins.
    pub(super) fn take_near(&self, at: Vec3, taken: &mut u8) -> Option<CachedPoint> {
        let free = |index: &usize| *taken & (1 << *index) == 0;
        let (index, gap) = self
            .points()
            .iter()
            .enumerate()
            .filter(|(index, _)| free(index))
            .map(|(index, point)| (index, (point.at - at).length_squared()))
            .min_by(|first, second| first.1.total_cmp(&second.1))?;
        if gap > WARM_MATCH_DISTANCE * WARM_MATCH_DISTANCE {
            return None;
        }
        *taken |= 1 << index;
        self.points().get(index).copied()
    }

    fn write_bits(&self, hasher: &mut StateHasher) {
        hasher.write_u32(self.first.index());
        hasher.write_u32(self.first.generation());
        hasher.write_u32(self.second.index());
        hasher.write_u32(self.second.generation());
        hasher.write_u8(u8::try_from(self.count).unwrap_or(u8::MAX));
        for point in self.points() {
            point.at.write_bits(hasher);
            point.tangent.write_bits(hasher);
            hasher.write_f32(point.normal);
        }
    }
}

/// A buffer of cached manifolds that never grows past its capacity.
pub(super) struct WarmCache {
    entries: Vec<CachedManifold>,
    capacity: usize,
}

impl WarmCache {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            capacity,
        }
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
    }

    /// Stores `entry` unless the buffer is full (it cannot be: one entry per manifold, and the
    /// capacity is the manifold capacity).
    pub(super) fn push(&mut self, entry: CachedManifold) {
        if self.entries.len() < self.capacity {
            self.entries.push(entry);
        }
    }

    /// The entry of the pair `(first, second)` at or after `*cursor`, moving the cursor past the
    /// entries of lower pairs. Asked for pairs in increasing order, as the manifold list is.
    pub(super) fn find(
        &self,
        cursor: &mut usize,
        first: BodyHandle<D3>,
        second: BodyHandle<D3>,
    ) -> Option<&CachedManifold> {
        let key = (first.index(), second.index());
        let lower = self
            .entries
            .get(*cursor..)?
            .iter()
            .take_while(|entry| (entry.first.index(), entry.second.index()) < key)
            .count();
        *cursor += lower;
        let entry = self.entries.get(*cursor)?;
        (entry.first == first && entry.second == second).then_some(entry)
    }

    pub(super) fn is_finite(&self) -> bool {
        self.entries
            .iter()
            .all(|entry| entry.points().iter().all(CachedPoint::is_finite))
    }

    /// Nothing when empty, so a world without contacts hashes as it did before the solver existed.
    pub(super) fn write_bits(&self, hasher: &mut StateHasher) {
        if self.entries.is_empty() {
            return;
        }
        hasher.write_u64(self.entries.len() as u64);
        for entry in &self.entries {
            entry.write_bits(hasher);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base::pool::Handle;

    fn h(index: u32, generation: u32) -> BodyHandle<D3> {
        Handle::from_raw(index, generation)
    }

    fn at(x: f32, normal: f32) -> CachedPoint {
        CachedPoint {
            at: Vec3::new(x, 0.0, 0.0),
            normal,
            tangent: Vec3::new(0.0, normal, 0.0),
        }
    }

    fn manifold(points: &[CachedPoint]) -> CachedManifold {
        CachedManifold::new(h(0, 0), h(1, 0), points)
    }

    fn hash_of(cache: &WarmCache) -> u64 {
        let mut hasher = StateHasher::new();
        cache.write_bits(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn a_point_takes_the_nearest_kept_point_within_reach_and_marks_it() {
        let kept = manifold(&[at(0.0, 1.0), at(0.5, 2.0), at(0.51, 3.0)]);
        let mut taken = 0_u8;
        let found = kept.take_near(Vec3::new(0.505, 0.0, 0.0), &mut taken);
        // 0.5 and 0.51 are equally near: the first one wins.
        assert_eq!(found.map(|p| p.normal), Some(2.0));
        assert_eq!(taken, 0b010);
        // The same place again takes the other one, never the same twice.
        let again = kept.take_near(Vec3::new(0.505, 0.0, 0.0), &mut taken);
        assert_eq!(again.map(|p| p.normal), Some(3.0));
        assert_eq!(taken, 0b110);
        // Nothing is left within reach of that place.
        assert_eq!(kept.take_near(Vec3::new(0.505, 0.0, 0.0), &mut taken), None);
    }

    #[test]
    fn a_point_that_moved_too_far_starts_from_zero() {
        let kept = manifold(&[at(0.0, 1.0)]);
        let reach = WARM_MATCH_DISTANCE;
        let mut taken = 0_u8;
        assert!(
            kept.take_near(Vec3::new(reach * 1.05, 0.0, 0.0), &mut taken)
                .is_none()
        );
        assert_eq!(taken, 0);
        assert!(
            kept.take_near(Vec3::new(reach * 0.95, 0.0, 0.0), &mut taken)
                .is_some()
        );
        assert_eq!(taken, 1);
    }

    #[test]
    fn a_point_exactly_at_the_reach_is_still_the_same_contact() {
        let kept = manifold(&[at(0.0, 1.0)]);
        let mut taken = 0_u8;
        assert!(
            kept.take_near(Vec3::new(WARM_MATCH_DISTANCE, 0.0, 0.0), &mut taken)
                .is_some()
        );
    }

    #[test]
    fn a_distance_counts_along_every_axis() {
        let kept = manifold(&[at(0.0, 1.0)]);
        let mut taken = 0_u8;
        let far = WARM_MATCH_DISTANCE * 0.8;
        assert!(
            kept.take_near(Vec3::new(far, far, far), &mut taken)
                .is_none()
        );
        let near = WARM_MATCH_DISTANCE * 0.5;
        assert!(
            kept.take_near(Vec3::new(near, near, near), &mut taken)
                .is_some()
        );
    }

    #[test]
    fn only_the_points_given_are_kept_up_to_the_manifold_limit() {
        let many: Vec<CachedPoint> = (0..6_u8).map(|n| at(f32::from(n), 1.0)).collect();
        let kept = manifold(&many);
        assert_eq!(kept.points().len(), MAX_MANIFOLD_POINTS);
        assert_eq!(manifold(&[]).points().len(), 0);
    }

    #[test]
    fn the_cursor_finds_pairs_in_increasing_order_and_skips_lower_ones() {
        let mut cache = WarmCache::new(4);
        for (a, b) in [(0, 1), (0, 2), (1, 2)] {
            cache.push(CachedManifold::new(h(a, 0), h(b, 0), &[at(0.0, 1.0)]));
        }
        let mut cursor = 0;
        assert!(cache.find(&mut cursor, h(0, 0), h(2, 0)).is_some());
        assert_eq!(cursor, 1);
        // A pair that is not there, between two that are.
        assert!(cache.find(&mut cursor, h(1, 0), h(1, 0)).is_none());
        assert!(cache.find(&mut cursor, h(1, 0), h(2, 0)).is_some());
        assert_eq!(cursor, 2);
        assert!(cache.find(&mut cursor, h(2, 0), h(3, 0)).is_none());
    }

    #[test]
    fn a_pair_with_another_generation_is_not_the_same_pair() {
        let mut cache = WarmCache::new(1);
        cache.push(CachedManifold::new(h(0, 0), h(1, 0), &[at(0.0, 1.0)]));
        assert!(cache.find(&mut 0, h(0, 1), h(1, 0)).is_none());
        assert!(cache.find(&mut 0, h(0, 0), h(1, 1)).is_none());
        assert!(cache.find(&mut 0, h(0, 0), h(1, 0)).is_some());
    }

    #[test]
    fn the_cache_never_grows_past_its_capacity_and_clear_empties_it() {
        let mut cache = WarmCache::new(2);
        for n in 0..5 {
            cache.push(CachedManifold::new(h(0, 0), h(n + 1, 0), &[]));
        }
        assert_eq!(cache.entries.len(), 2);
        cache.clear();
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn an_empty_cache_hashes_nothing_and_a_full_one_hashes_every_field() {
        let empty = StateHasher::new().finish();
        let mut cache = WarmCache::new(2);
        assert_eq!(hash_of(&cache), empty);
        cache.push(manifold(&[at(0.0, 1.0)]));
        let base = hash_of(&cache);
        assert_ne!(base, empty);
        let changed = |edit: &dyn Fn(&mut CachedPoint)| {
            let mut point = at(0.0, 1.0);
            edit(&mut point);
            let mut other = WarmCache::new(2);
            other.push(manifold(&[point]));
            hash_of(&other)
        };
        assert_ne!(changed(&|p| p.at.y = 0.25), base);
        assert_ne!(changed(&|p| p.at.z = 0.25), base);
        assert_ne!(changed(&|p| p.normal = 2.0), base);
        assert_ne!(changed(&|p| p.tangent.z = 0.25), base);
        let mut other = WarmCache::new(2);
        other.push(CachedManifold::new(h(0, 0), h(2, 0), &[at(0.0, 1.0)]));
        assert_ne!(hash_of(&other), base);
        other.clear();
        other.push(CachedManifold::new(h(0, 3), h(1, 0), &[at(0.0, 1.0)]));
        assert_ne!(hash_of(&other), base);
    }

    #[test]
    fn a_part_that_is_not_a_number_makes_the_cache_not_finite() {
        let mut cache = WarmCache::new(1);
        cache.push(manifold(&[at(0.0, 1.0)]));
        assert!(cache.is_finite());
        for bad in [
            CachedPoint {
                at: Vec3::new(f32::NAN, 0.0, 0.0),
                ..at(0.0, 1.0)
            },
            CachedPoint {
                normal: f32::INFINITY,
                ..at(0.0, 1.0)
            },
            CachedPoint {
                tangent: Vec3::new(0.0, 0.0, f32::NAN),
                ..at(0.0, 1.0)
            },
        ] {
            let mut cache = WarmCache::new(1);
            cache.push(manifold(&[bad]));
            assert!(!cache.is_finite());
        }
    }
}
