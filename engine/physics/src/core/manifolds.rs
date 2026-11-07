//! The manifold buffer: one contact manifold per touching pair, in the order of the pair list, in a
//! buffer of fixed capacity. Manifolds past the capacity are counted, not stored, exactly as the
//! pair list does it ([`ManifoldList::missing`], [`ManifoldList::is_complete`]); nothing panics and
//! nothing is lost without being counted.

use super::MAX_PAIRS;
use super::body::BodyHandle;
use super::dim::Dim;
use super::error::PhysicsError;
use super::hash::{HashBits, StateHasher};

/// The manifold of two bodies. `first` and `second` are the pair's bodies (the lower slot index
/// first) and the manifold's normal points from `first` to `second`.
pub struct ContactManifold<D: Dim> {
    /// The body the normal points away from.
    pub first: BodyHandle<D>,
    /// The body the normal points to.
    pub second: BodyHandle<D>,
    /// The contact normal and points.
    pub manifold: D::Manifold,
}

impl<D: Dim> Clone for ContactManifold<D> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<D: Dim> Copy for ContactManifold<D> {}

impl<D: Dim> PartialEq for ContactManifold<D> {
    fn eq(&self, other: &Self) -> bool {
        self.first == other.first && self.second == other.second && self.manifold == other.manifold
    }
}

impl<D: Dim> std::fmt::Debug for ContactManifold<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContactManifold")
            .field("first", &self.first)
            .field("second", &self.second)
            .field("manifold", &self.manifold)
            .finish()
    }
}

/// The manifolds of one step, in the order of the pair list, in a buffer that never grows.
pub struct ManifoldList<D: Dim> {
    entries: Vec<ContactManifold<D>>,
    capacity: usize,
    missing: usize,
}

impl<D: Dim> ManifoldList<D> {
    /// A list that holds at most `capacity` manifolds; allocates once, here.
    ///
    /// # Errors
    /// [`PhysicsError::ManifoldCapacityTooLarge`] when `capacity` is above [`MAX_PAIRS`].
    pub fn new(capacity: u32) -> Result<Self, PhysicsError> {
        if capacity > MAX_PAIRS {
            return Err(PhysicsError::ManifoldCapacityTooLarge { max: MAX_PAIRS });
        }
        Ok(Self {
            entries: Vec::with_capacity(capacity as usize),
            capacity: capacity as usize,
            missing: 0,
        })
    }

    /// The most manifolds the list holds.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// The number of manifolds stored.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when no manifold is stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The stored manifolds, in the order of the pair list.
    #[must_use]
    pub fn as_slice(&self) -> &[ContactManifold<D>] {
        &self.entries
    }

    /// How many manifolds the last computation made that did not fit. They are the ones of the
    /// last pairs of the pair list (the list is filled in order), so the stored ones are a prefix.
    #[must_use]
    pub fn missing(&self) -> usize {
        self.missing
    }

    /// True when every manifold of the last computation is stored.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.missing == 0
    }

    /// A hash of the stored manifolds (handles and manifold bits, in order) and of the count of
    /// missing ones; equal inputs give equal hashes. It is not part of the state hash of the world.
    #[must_use]
    pub fn hash(&self) -> u64 {
        let mut hasher = StateHasher::new();
        hasher.write_u64(self.entries.len() as u64);
        hasher.write_u64(self.missing as u64);
        for entry in &self.entries {
            hasher.write_u32(entry.first.index());
            hasher.write_u32(entry.first.generation());
            hasher.write_u32(entry.second.index());
            hasher.write_u32(entry.second.generation());
            entry.manifold.write_bits(&mut hasher);
        }
        hasher.finish()
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.missing = 0;
    }

    pub(super) fn push(&mut self, entry: ContactManifold<D>) {
        if self.entries.len() >= self.capacity {
            self.missing += 1;
            return;
        }
        self.entries.push(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::d2::{D2, Manifold2};
    use base::pool::Handle;

    fn h(index: u32) -> BodyHandle<D2> {
        Handle::from_raw(index, 0)
    }

    fn entry(first: u32, second: u32) -> ContactManifold<D2> {
        ContactManifold {
            first: h(first),
            second: h(second),
            manifold: Manifold2::default(),
        }
    }

    fn list(capacity: u32) -> ManifoldList<D2> {
        match ManifoldList::new(capacity) {
            Ok(list) => list,
            Err(error) => panic!("list must build: {error}"),
        }
    }

    #[test]
    fn two_entries_are_equal_only_when_both_handles_and_the_manifold_are() {
        let plain = entry(0, 1);
        assert_eq!(plain, entry(0, 1));
        assert_ne!(plain, entry(2, 1));
        assert_ne!(plain, entry(0, 2));
        let mut other = plain;
        other.manifold.normal = base::math::Vec2::new(0.0, 1.0);
        assert_ne!(plain, other);
    }

    #[test]
    fn an_entry_prints_its_handles_and_its_manifold() {
        let text = format!("{:?}", entry(0, 1));
        assert!(text.starts_with("ContactManifold"), "{text}");
        assert!(text.contains("first") && text.contains("second") && text.contains("manifold"));
    }

    #[test]
    fn a_new_list_is_empty_complete_and_has_its_capacity() {
        let l = list(4);
        assert_eq!((l.capacity(), l.len(), l.is_empty()), (4, 0, true));
        assert_eq!((l.missing(), l.is_complete()), (0, true));
        assert!(l.as_slice().is_empty());
    }

    #[test]
    fn a_capacity_above_the_maximum_is_refused() {
        assert_eq!(
            ManifoldList::<D2>::new(MAX_PAIRS + 1).err(),
            Some(PhysicsError::ManifoldCapacityTooLarge { max: MAX_PAIRS })
        );
        assert!(ManifoldList::<D2>::new(MAX_PAIRS).is_ok());
    }

    #[test]
    fn manifolds_are_stored_in_the_order_pushed() {
        let mut l = list(4);
        l.push(entry(0, 1));
        l.push(entry(0, 2));
        assert_eq!(l.as_slice(), &[entry(0, 1), entry(0, 2)]);
        assert!(!l.is_empty());
    }

    #[test]
    fn a_manifold_past_the_capacity_is_counted_not_stored_and_clear_resets_the_count() {
        let mut l = list(2);
        for i in 1..=5 {
            l.push(entry(0, i));
        }
        assert_eq!((l.len(), l.missing(), l.is_complete()), (2, 3, false));
        assert_eq!(l.as_slice(), &[entry(0, 1), entry(0, 2)]);
        l.clear();
        assert_eq!((l.len(), l.missing(), l.is_complete()), (0, 0, true));
        assert_eq!(l.capacity(), 2);
    }

    #[test]
    fn a_list_filled_to_exactly_its_capacity_is_complete() {
        let mut l = list(2);
        l.push(entry(0, 1));
        l.push(entry(0, 2));
        assert_eq!((l.len(), l.missing(), l.is_complete()), (2, 0, true));
        l.push(entry(0, 3));
        assert_eq!((l.len(), l.missing(), l.is_complete()), (2, 1, false));
    }

    #[test]
    fn the_hash_covers_order_handles_count_and_missing() {
        let mut left = list(4);
        let mut right = list(4);
        assert_eq!(left.hash(), right.hash());
        left.push(entry(0, 1));
        assert_ne!(left.hash(), right.hash());
        right.push(entry(0, 1));
        assert_eq!(left.hash(), right.hash());
        left.push(entry(0, 2));
        left.push(entry(1, 2));
        right.push(entry(1, 2));
        right.push(entry(0, 2));
        assert_ne!(left.hash(), right.hash());
        // The second handle, and a generation, are hashed too.
        let mut one = list(4);
        let mut other = list(4);
        one.push(entry(0, 1));
        other.push(entry(0, 2));
        assert_ne!(one.hash(), other.hash());
        let mut aged = list(4);
        aged.push(ContactManifold {
            first: Handle::from_raw(0, 1),
            ..entry(0, 1)
        });
        assert_ne!(aged.hash(), right.hash());
        // A missing manifold changes the hash even when the stored ones are the same.
        let mut full = list(1);
        full.push(entry(0, 1));
        let stored = full.hash();
        full.push(entry(0, 2));
        assert_ne!(full.hash(), stored);
    }

    #[test]
    fn the_hash_covers_the_second_generation_and_the_manifold() {
        let mut f = list(2);
        f.push(ContactManifold {
            second: Handle::from_raw(1, 7),
            ..entry(0, 1)
        });
        let mut g = list(2);
        g.push(entry(0, 1));
        assert_ne!(f.hash(), g.hash());
        let mut moved = entry(0, 1);
        moved.manifold.normal = base::math::Vec2::new(0.0, 1.0);
        let mut m = list(2);
        m.push(moved);
        assert_ne!(m.hash(), g.hash());
    }
}
