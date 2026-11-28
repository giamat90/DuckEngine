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
    /// Scratch of the counting sort, allocated once by [`PairList::reserve_slots`].
    buckets: Buckets,
}

/// Where the pairs of each lower slot index begin, and the next free place in each such bucket.
#[derive(Default)]
struct Buckets {
    /// `starts[i]` is where the pairs whose lower slot is `i` begin; the last entry is the length.
    starts: Vec<u32>,
    /// `heads[i]` is the next place to fill while the pairs are put in their buckets.
    heads: Vec<u32>,
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
            buckets: Buckets::default(),
        })
    }

    /// Makes room for the counting sort of pairs whose slot indices are below `slots`. Allocates
    /// once; without it (or for a pair above it) [`PairList::sort`] uses the heap sort, which gives
    /// the same order.
    pub(super) fn reserve_slots(&mut self, slots: u32) {
        let slots = slots as usize;
        self.buckets.starts = vec![0; slots + 1];
        self.buckets.heads = vec![0; slots];
    }

    /// The number of slot indices the counting sort has room for.
    #[cfg(test)]
    pub(super) fn reserved_slots(&self) -> usize {
        self.buckets.heads.len()
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
    /// so the order is total, and every sort that follows it gives the same list: the counting sort
    /// (cards 8-114) when the slots are reserved, the heap sort otherwise.
    pub(super) fn sort(&mut self) {
        if !self.sort_by_counting() {
            heap_sort_by_key(&mut self.pairs, Pair::key);
        }
    }

    /// Counts the pairs of each lower slot index, puts them in their buckets in place, then sorts
    /// each bucket by the higher index. Returns false, having changed nothing that matters, when a
    /// pair has a slot index the scratch has no room for.
    fn sort_by_counting(&mut self) -> bool {
        if !self.count_buckets() {
            return false;
        }
        self.fill_buckets();
        self.sort_buckets();
        true
    }

    /// Fills `starts` with the first place of each bucket and `heads` with the same places.
    fn count_buckets(&mut self) -> bool {
        let Buckets { starts, heads } = &mut self.buckets;
        starts.fill(0);
        if !self
            .pairs
            .iter()
            .all(|pair| count_in_bucket(starts, pair.first.index()))
        {
            return false;
        }
        running_sum(starts);
        heads
            .iter_mut()
            .zip(starts.iter())
            .for_each(|(head, start)| *head = *start);
        true
    }

    /// Moves every pair to its bucket by cycles of swaps; `heads` ends at the end of each bucket.
    fn fill_buckets(&mut self) {
        for bucket in 0..self.buckets.heads.len() {
            self.fill_bucket(bucket);
        }
    }

    /// Places pairs until the bucket holds only its own.
    fn fill_bucket(&mut self, bucket: usize) {
        while self.place_next(bucket) {}
    }

    /// Takes the next unplaced pair of `bucket` to the next free place of its own bucket (a pair of
    /// this bucket swaps with itself, so it stays). False when the bucket is full.
    fn place_next(&mut self, bucket: usize) -> bool {
        let Buckets { starts, heads } = &mut self.buckets;
        let head = heads[bucket] as usize;
        if head >= starts[bucket + 1] as usize {
            return false;
        }
        let target = self.pairs[head].first.index() as usize;
        self.pairs.swap(head, heads[target] as usize);
        heads[target] += 1;
        true
    }

    /// Sorts each bucket by the higher slot index.
    fn sort_buckets(&mut self) {
        for bucket in self.buckets.starts.windows(2) {
            sort_by_higher(self.pairs.get_mut(bucket[0] as usize..bucket[1] as usize));
        }
    }
}

/// Counts one more pair for the bucket of slot `index`, kept one place up so that the running sum
/// turns the counts into first places. False when the scratch has no bucket for `index`.
fn count_in_bucket(starts: &mut [u32], index: u32) -> bool {
    starts
        .get_mut(index as usize + 1)
        .map(|count| *count += 1)
        .is_some()
}

/// Replaces each value by the sum of the values up to and including it.
fn running_sum(values: &mut [u32]) {
    let mut total = 0;
    for value in values {
        total += *value;
        *value = total;
    }
}

/// Sorts the pairs of one bucket by the higher slot index.
fn sort_by_higher<D: Dim>(bucket: Option<&mut [Pair<D>]>) {
    if let Some(pairs) = bucket {
        heap_sort_by_key(pairs, |pair| pair.second.index());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::d2::D2;
    use base::pool::Handle;
    use base::rng::Pcg32;

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

    /// `count` distinct pairs of slot indices below `slots`, in a scrambled order.
    fn scrambled(rng: &mut Pcg32, slots: u32, count: usize) -> Vec<(u32, u32)> {
        let mut seen = std::collections::BTreeSet::new();
        while seen.len() < count {
            seen.insert(random_pair(rng, slots));
        }
        let mut pairs: Vec<(u32, u32)> = seen.into_iter().collect();
        // A deterministic shuffle (Fisher-Yates) so the list arrives out of order.
        for i in (1..pairs.len()).rev() {
            pairs.swap(i, below(rng, u32::try_from(i + 1).unwrap_or(1)) as usize);
        }
        pairs
    }

    fn below(rng: &mut Pcg32, bound: u32) -> u32 {
        rng.below(bound).unwrap_or(0)
    }

    /// Two different slot indices below `slots`, the lower one first (a pair may be the same again).
    fn random_pair(rng: &mut Pcg32, slots: u32) -> (u32, u32) {
        let a = below(rng, slots);
        let b = (a + 1 + below(rng, slots - 1)) % slots;
        (a.min(b), a.max(b))
    }

    fn sorted_indices(l: &PairList<D2>) -> Vec<(u32, u32)> {
        l.as_slice()
            .iter()
            .map(|p| (p.first.index(), p.second.index()))
            .collect()
    }

    fn push_all(l: &mut PairList<D2>, pairs: &[(u32, u32)]) {
        for &(a, b) in pairs {
            l.push(h(a), h(b));
        }
    }

    fn filled(slots: Option<u32>, pairs: &[(u32, u32)]) -> PairList<D2> {
        let mut l = list(u32::try_from(pairs.len()).unwrap_or(0));
        if let Some(slots) = slots {
            l.reserve_slots(slots);
        }
        push_all(&mut l, pairs);
        l
    }

    fn counting_equals_standard_of_every_count(rng: &mut Pcg32, slots: u32) {
        let most = (slots * (slots - 1) / 2).min(300) as usize;
        for count in [0, 1, most / 2, most] {
            counting_equals_standard(rng, slots, count);
        }
    }

    fn counting_equals_standard(rng: &mut Pcg32, slots: u32, count: usize) {
        let input = scrambled(rng, slots, count);
        let mut expected = input.clone();
        expected.sort_by_key(|&pair| pair);
        let mut l = filled(Some(slots), &input);
        assert!(l.sort_by_counting(), "slots {slots} count {count}");
        assert_eq!(sorted_indices(&l), expected, "slots {slots} count {count}");
    }

    #[test]
    fn the_counting_sort_gives_the_order_of_the_standard_sort_for_every_size() {
        let mut rng = Pcg32::new(114, 1);
        for slots in [2_u32, 3, 10, 64, 200] {
            counting_equals_standard_of_every_count(&mut rng, slots);
        }
    }

    #[test]
    fn one_bucket_holding_most_of_the_pairs_is_sorted_by_the_higher_index() {
        // A ground body in slot 0 with every other body on it, and a few pairs elsewhere.
        let mut input: Vec<(u32, u32)> = (1..40).rev().map(|i| (0, i)).collect();
        input.extend([(5, 9), (2, 3), (5, 6)]);
        let mut expected = input.clone();
        expected.sort_by_key(|&pair| pair);
        let mut l = filled(Some(40), &input);
        l.sort();
        assert_eq!(sorted_indices(&l), expected);
    }

    #[test]
    fn a_pair_above_the_reserved_slots_falls_back_to_the_heap_sort() {
        // The bucket is the lower index: (3, 8) fits five slots, (7, 9) does not.
        let input = [(7, 9), (1, 2), (3, 4), (0, 9)];
        let mut l = filled(Some(5), &input);
        assert!(!l.sort_by_counting());
        l.sort();
        assert_eq!(sorted_indices(&l), vec![(0, 9), (1, 2), (3, 4), (7, 9)]);
    }

    #[test]
    fn without_reserved_slots_the_list_is_sorted_by_the_heap_sort() {
        let input = [(3, 8), (1, 2), (3, 4), (0, 9)];
        let mut l = filled(None, &input);
        assert!(!l.sort_by_counting());
        l.sort();
        assert_eq!(sorted_indices(&l), vec![(0, 9), (1, 2), (3, 4), (3, 8)]);
        assert_eq!(l.reserved_slots(), 0);
    }

    #[test]
    fn a_list_is_sorted_again_after_it_is_cleared_and_refilled() {
        let mut l = list(8);
        l.reserve_slots(10);
        assert_eq!(l.reserved_slots(), 10);
        for round in [[(4, 5), (1, 2), (1, 9)], [(7, 8), (0, 3), (0, 1)]] {
            l.clear();
            push_all(&mut l, &round);
            l.sort();
            let mut expected = round.to_vec();
            expected.sort_by_key(|&pair| pair);
            assert_eq!(sorted_indices(&l), expected);
        }
    }

    #[test]
    fn an_empty_list_and_a_zero_capacity_list_are_sorted() {
        let mut l = list(0);
        l.reserve_slots(0);
        l.sort();
        assert!(l.is_empty());
        let mut l = filled(Some(4), &[]);
        assert!(l.sort_by_counting());
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
