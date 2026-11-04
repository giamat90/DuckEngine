//! The pair list: the bodies whose fat boxes overlap, in one canonical order, in a buffer of fixed
//! capacity.
//!
//! A pair names its two bodies by handle, the one with the lower slot index first. The list is sorted
//! by (lower slot index, higher slot index), a total order on distinct pairs, so it is the same for
//! the same bodies on every run and at any thread count: it never depends on the order in which the
//! tree was walked, on an address or on a hash. Pairs of two static bodies never appear.
//!
//! The buffer never grows. A pair that does not fit is not stored and is counted: [`PairList::missing`]
//! is the number of pairs the buffer could not take and [`PairList::is_complete`] is false. The
//! stored pairs are then a deterministic but arbitrary subset (dynamic bodies in slot order, for each the hits in the dynamic tree and then in the static tree), so a
//! caller that needs every pair must treat an incomplete list as an error and build a world with a
//! larger `max_pairs`.

use super::MAX_PAIRS;
use super::body::BodyHandle;
use super::dim::Dim;
use super::error::PhysicsError;
use super::sort::heap_sort_by_key;

/// Two bodies whose fat boxes overlap, `first` having the lower slot index.
pub struct Pair<D: Dim> {
    /// The body with the lower slot index.
    pub first: BodyHandle<D>,
    /// The body with the higher slot index.
    pub second: BodyHandle<D>,
}

// Written by hand: `#[derive]` would add the bounds `D: Clone`, `D: PartialEq`, `D: Debug`
// (ADR-0018, Rust friction 3).
impl<D: Dim> Clone for Pair<D> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<D: Dim> Copy for Pair<D> {}

impl<D: Dim> PartialEq for Pair<D> {
    fn eq(&self, other: &Self) -> bool {
        self.first == other.first && self.second == other.second
    }
}

impl<D: Dim> Eq for Pair<D> {}

impl<D: Dim> std::fmt::Debug for Pair<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pair")
            .field("first", &self.first)
            .field("second", &self.second)
            .finish()
    }
}

impl<D: Dim> Pair<D> {
    /// The sort key: the two slot indices, lower one in the high half.
    fn key(&self) -> u64 {
        (u64::from(self.first.index()) << 32) | u64::from(self.second.index())
    }
}

/// The pairs found by the last broadphase pass.
pub struct PairList<D: Dim> {
    pairs: Vec<Pair<D>>,
    capacity: usize,
    missing: usize,
}

impl<D: Dim> PairList<D> {
    /// A list that holds up to `capacity` pairs. Allocates the buffer once.
    ///
    /// # Errors
    /// [`PhysicsError::PairCapacityTooLarge`] above [`MAX_PAIRS`].
    pub fn new(capacity: u32) -> Result<Self, PhysicsError> {
        if capacity > MAX_PAIRS {
            return Err(PhysicsError::PairCapacityTooLarge { max: MAX_PAIRS });
        }
        Ok(Self {
            pairs: Vec::with_capacity(capacity as usize),
            capacity: capacity as usize,
            missing: 0,
        })
    }

    /// The most pairs the list can hold.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// The number of pairs stored.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    /// True when no pair is stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// The stored pairs in canonical order (when the list is complete; see the module comment).
    #[must_use]
    pub fn as_slice(&self) -> &[Pair<D>] {
        &self.pairs
    }

    /// The number of overlapping pairs that did not fit and were not stored.
    #[must_use]
    pub fn missing(&self) -> usize {
        self.missing
    }

    /// True when every overlapping pair is stored (nothing is missing).
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.missing == 0
    }

    /// Empties the list and clears the missing count; keeps the buffer.
    pub(super) fn clear(&mut self) {
        self.pairs.clear();
        self.missing = 0;
    }

    /// Adds the pair of two different bodies, the one with the lower slot index first; counts it as
    /// missing when the buffer is full.
    pub(super) fn push(&mut self, a: BodyHandle<D>, b: BodyHandle<D>) {
        if self.pairs.len() >= self.capacity {
            self.missing += 1;
            return;
        }
        let (first, second) = if a.index() < b.index() {
            (a, b)
        } else {
            (b, a)
        };
        self.pairs.push(Pair { first, second });
    }

    /// Puts the stored pairs in canonical order. The keys are unique (a body pair is stored once),
    /// so the order is total.
    pub(super) fn sort(&mut self) {
        heap_sort_by_key(&mut self.pairs, Pair::key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::d2::D2;
    use base::pool::Handle;

    type H = BodyHandle<D2>;

    fn h(index: u32) -> H {
        Handle::from_raw(index, 0)
    }

    fn list(capacity: u32) -> PairList<D2> {
        match PairList::new(capacity) {
            Ok(list) => list,
            Err(error) => panic!("list must build: {error}"),
        }
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
            PairList::<D2>::new(MAX_PAIRS + 1).err(),
            Some(PhysicsError::PairCapacityTooLarge { max: MAX_PAIRS })
        );
    }

    #[test]
    fn a_pair_is_stored_with_the_lower_slot_index_first() {
        let mut l = list(4);
        l.push(h(7), h(2));
        l.push(h(1), h(9));
        assert_eq!(
            l.as_slice(),
            &[
                Pair {
                    first: h(2),
                    second: h(7)
                },
                Pair {
                    first: h(1),
                    second: h(9)
                }
            ]
        );
    }

    #[test]
    fn sorting_orders_by_the_lower_index_then_the_higher_one() {
        let mut l = list(8);
        for (a, b) in [(3, 4), (1, 9), (1, 2), (0, 5), (3, 2), (0, 1)] {
            l.push(h(a), h(b));
        }
        l.sort();
        let order: Vec<(u32, u32)> = l
            .as_slice()
            .iter()
            .map(|p| (p.first.index(), p.second.index()))
            .collect();
        assert_eq!(order, vec![(0, 1), (0, 5), (1, 2), (1, 9), (2, 3), (3, 4)]);
    }

    #[test]
    fn the_key_puts_the_lower_index_in_the_high_half() {
        let pair = Pair::<D2> {
            first: h(3),
            second: h(5),
        };
        assert_eq!(pair.key(), (3_u64 << 32) | 5);
    }

    #[test]
    fn a_pair_past_the_capacity_is_counted_not_stored_and_clear_resets_the_count() {
        let mut l = list(2);
        for i in 1..=5 {
            l.push(h(0), h(i));
        }
        assert_eq!((l.len(), l.missing(), l.is_complete()), (2, 3, false));
        l.clear();
        assert_eq!((l.len(), l.missing(), l.is_complete()), (0, 0, true));
        assert_eq!(l.capacity(), 2);
    }

    #[test]
    fn a_list_filled_to_exactly_its_capacity_is_complete_and_not_empty() {
        let mut l = list(2);
        l.push(h(0), h(1));
        assert!(!l.is_empty());
        l.push(h(0), h(2));
        assert_eq!((l.len(), l.missing(), l.is_complete()), (2, 0, true));
        l.push(h(0), h(3));
        assert_eq!((l.len(), l.missing(), l.is_complete()), (2, 1, false));
    }

    #[test]
    fn a_list_of_zero_capacity_counts_every_pair_as_missing() {
        let mut l = list(0);
        l.push(h(0), h(1));
        assert_eq!((l.len(), l.missing()), (0, 1));
    }

    #[test]
    fn pairs_compare_by_both_handles_and_print_them() {
        let a = Pair::<D2> {
            first: h(1),
            second: h(2),
        };
        let same = a;
        let other_second = Pair::<D2> {
            first: h(1),
            second: h(3),
        };
        let other_first = Pair::<D2> {
            first: h(0),
            second: h(2),
        };
        assert_eq!(a, same);
        assert_ne!(a, other_second);
        assert_ne!(a, other_first);
        assert!(format!("{a:?}").starts_with("Pair"));
    }
}
