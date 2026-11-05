//! The warm start: the impulses of the last step's contacts, kept by (pair, feature id) so that the
//! next step starts from them. The manifold list is in pair order, so the old entries are matched
//! with a cursor that only moves forward: no search, no hash map, the same result on every run.

use crate::core::{BodyHandle, StateHasher};
use crate::d2::{D2, FeatureId, MAX_MANIFOLD_POINTS};

/// The accumulated impulses of one contact point.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Impulse {
    /// Along the normal, zero or more.
    pub normal: f32,
    /// Along the tangent, either sign.
    pub tangent: f32,
}

impl Impulse {
    fn is_finite(self) -> bool {
        self.normal.is_finite() && self.tangent.is_finite()
    }
}

/// The impulse of one point and the feature it belongs to.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct CachedPoint {
    pub id: FeatureId,
    pub impulse: Impulse,
}

/// The impulses of one manifold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CachedManifold {
    pub first: BodyHandle<D2>,
    pub second: BodyHandle<D2>,
    count: usize,
    points: [CachedPoint; MAX_MANIFOLD_POINTS],
}

impl CachedManifold {
    pub(super) fn new(
        first: BodyHandle<D2>,
        second: BodyHandle<D2>,
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

    /// The impulse kept for the point with feature `id`; zero for a feature not seen before.
    pub(super) fn impulse_of(&self, id: FeatureId) -> Impulse {
        self.points()
            .iter()
            .find(|point| point.id == id)
            .map_or_else(Impulse::default, |point| point.impulse)
    }

    fn write_bits(&self, hasher: &mut StateHasher) {
        hasher.write_u32(self.first.index());
        hasher.write_u32(self.first.generation());
        hasher.write_u32(self.second.index());
        hasher.write_u32(self.second.generation());
        hasher.write_u8(u8::try_from(self.count).unwrap_or(u8::MAX));
        for point in self.points() {
            hasher.write_u32(u32::from(point.id.raw()));
            hasher.write_f32(point.impulse.normal);
            hasher.write_f32(point.impulse.tangent);
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
        first: BodyHandle<D2>,
        second: BodyHandle<D2>,
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
            .all(|entry| entry.points().iter().all(|point| point.impulse.is_finite()))
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

    fn h(index: u32, generation: u32) -> BodyHandle<D2> {
        Handle::from_raw(index, generation)
    }

    fn point(kind: u8, normal: f32, tangent: f32) -> CachedPoint {
        CachedPoint {
            id: FeatureId::pack(kind, 0, 0),
            impulse: Impulse { normal, tangent },
        }
    }

    fn hash_of(cache: &WarmCache) -> u64 {
        let mut hasher = StateHasher::new();
        cache.write_bits(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn an_impulse_is_found_by_its_feature_id_and_a_new_feature_starts_at_zero() {
        let entry =
            CachedManifold::new(h(0, 0), h(1, 0), &[point(1, 2.0, -0.5), point(2, 3.0, 0.5)]);
        let second = entry.impulse_of(FeatureId::pack(2, 0, 0));
        assert_eq!((second.normal, second.tangent), (3.0, 0.5));
        assert_eq!(
            entry.impulse_of(FeatureId::pack(1, 0, 0)).tangent.to_bits(),
            (-0.5_f32).to_bits()
        );
        assert_eq!(
            entry.impulse_of(FeatureId::pack(3, 0, 0)),
            Impulse::default()
        );
    }

    #[test]
    fn only_the_points_given_are_kept_up_to_the_manifold_limit() {
        let many = [point(1, 1.0, 0.0); MAX_MANIFOLD_POINTS + 2];
        let entry = CachedManifold::new(h(0, 0), h(1, 0), &many);
        assert_eq!(entry.points().len(), MAX_MANIFOLD_POINTS);
        let none = CachedManifold::new(h(0, 0), h(1, 0), &[]);
        assert!(none.points().is_empty());
        assert_eq!(
            none.impulse_of(FeatureId::pack(1, 0, 0)),
            Impulse::default()
        );
    }

    #[test]
    fn the_cursor_finds_pairs_in_increasing_order_and_skips_lower_ones() {
        let mut cache = WarmCache::new(4);
        cache.push(CachedManifold::new(h(0, 0), h(1, 0), &[point(1, 1.0, 0.0)]));
        cache.push(CachedManifold::new(h(0, 0), h(3, 0), &[point(1, 2.0, 0.0)]));
        cache.push(CachedManifold::new(h(2, 0), h(3, 0), &[point(1, 3.0, 0.0)]));
        let mut cursor = 0;
        assert!(cache.find(&mut cursor, h(0, 0), h(2, 0)).is_none());
        let found = cache.find(&mut cursor, h(0, 0), h(3, 0));
        assert_eq!(
            found.map(|e| e.impulse_of(FeatureId::pack(1, 0, 0)).normal),
            Some(2.0)
        );
        assert_eq!(cursor, 1);
        assert!(cache.find(&mut cursor, h(2, 0), h(4, 0)).is_none());
        assert!(cache.find(&mut cursor, h(5, 0), h(6, 0)).is_none());
        assert_eq!(cursor, 3);
    }

    #[test]
    fn a_pair_with_another_generation_is_not_the_same_pair() {
        let mut cache = WarmCache::new(1);
        cache.push(CachedManifold::new(h(0, 0), h(1, 0), &[point(1, 1.0, 0.0)]));
        let mut cursor = 0;
        assert!(cache.find(&mut cursor, h(0, 1), h(1, 0)).is_none());
        let mut cursor = 0;
        assert!(cache.find(&mut cursor, h(0, 0), h(1, 1)).is_none());
        let mut cursor = 0;
        assert!(cache.find(&mut cursor, h(0, 0), h(1, 0)).is_some());
    }

    #[test]
    fn the_cache_never_grows_past_its_capacity_and_clear_empties_it() {
        let mut cache = WarmCache::new(1);
        cache.push(CachedManifold::new(h(0, 0), h(1, 0), &[point(1, 1.0, 0.0)]));
        let one = hash_of(&cache);
        cache.push(CachedManifold::new(h(0, 0), h(2, 0), &[point(1, 5.0, 0.0)]));
        assert_eq!(hash_of(&cache), one);
        cache.clear();
        let mut cursor = 0;
        assert!(cache.find(&mut cursor, h(0, 0), h(1, 0)).is_none());
    }

    #[test]
    fn an_empty_cache_hashes_nothing_and_a_full_one_hashes_every_field() {
        let mut cache = WarmCache::new(2);
        assert_eq!(hash_of(&cache), StateHasher::new().finish());
        let p = point(1, 1.0, 0.25);
        cache.push(CachedManifold::new(h(0, 0), h(1, 0), &[p]));
        let base = hash_of(&cache);
        assert_ne!(base, StateHasher::new().finish());
        let variants = [
            CachedManifold::new(h(2, 0), h(1, 0), &[p]),
            CachedManifold::new(h(0, 1), h(1, 0), &[p]),
            CachedManifold::new(h(0, 0), h(3, 0), &[p]),
            CachedManifold::new(h(0, 0), h(1, 1), &[p]),
            CachedManifold::new(h(0, 0), h(1, 0), &[point(2, 1.0, 0.25)]),
            CachedManifold::new(h(0, 0), h(1, 0), &[point(1, 1.5, 0.25)]),
            CachedManifold::new(h(0, 0), h(1, 0), &[point(1, 1.0, 0.5)]),
            CachedManifold::new(h(0, 0), h(1, 0), &[p, point(2, 0.0, 0.0)]),
        ];
        for variant in variants {
            let mut other = WarmCache::new(2);
            other.push(variant);
            assert_ne!(hash_of(&other), base, "{variant:?}");
        }
    }

    #[test]
    fn a_non_finite_impulse_makes_the_cache_not_finite() {
        let mut cache = WarmCache::new(2);
        assert!(cache.is_finite());
        cache.push(CachedManifold::new(h(0, 0), h(1, 0), &[point(1, 1.0, 0.0)]));
        assert!(cache.is_finite());
        for (n, t) in [(f32::NAN, 0.0), (1.0, f32::INFINITY)] {
            let mut bad = WarmCache::new(1);
            bad.push(CachedManifold::new(h(0, 0), h(1, 0), &[point(1, n, t)]));
            assert!(!bad.is_finite());
        }
    }
}
