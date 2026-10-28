//! A fixed-capacity hash map whose iteration order depends only on the operations done.
//!
//! [`DenseMap`] keeps its entries in one dense array in the order they were inserted;
//! removing an entry moves the last entry into the hole (swap-remove). A small open-addressing
//! table of `u32` positions finds an entry from its key. Iteration walks the dense array, so
//! the order is a function of the sequence of operations alone: it does not depend on the
//! capacity, on the process, on the thread count or on the platform (the hash is integer
//! arithmetic only, see [`hash64`]). That is the property simulation code needs from a map
//! (`wiki/architecture/physics.md` section 3, rule 3). The table and the entries are allocated
//! once, when the map is built; insert, remove and lookup make no allocator call afterwards.
//! Failure to fit is a value.

use crate::pool::Handle;

/// The `MurmurHash3` 64-bit finalizer: every input bit changes about half of the output bits, so keys
/// that differ only in their high bits (`i << 32`) or form a counter spread over the table.
/// Integer arithmetic only, so it gives the same bits on every platform.
#[must_use]
pub fn hash64(key: u64) -> u64 {
    let mut h = key;
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^ (h >> 33)
}

/// A key a [`DenseMap`] can hash: a plain value with an integer hash.
pub trait MapKey: Copy + Eq {
    /// The 64-bit hash of the key; equal keys must have equal hashes (a correctness requirement;
    /// a poor hash only slows lookups, it never changes the iteration order).
    fn hash_key(&self) -> u64;
}

impl MapKey for u32 {
    fn hash_key(&self) -> u64 {
        hash64(u64::from(*self))
    }
}

impl MapKey for u64 {
    fn hash_key(&self) -> u64 {
        hash64(*self)
    }
}

impl<T> MapKey for Handle<T> {
    fn hash_key(&self) -> u64 {
        hash64(u64::from(self.index()) | (u64::from(self.generation()) << 32))
    }
}

/// Table value of a slot that holds no entry.
const EMPTY: u32 = u32::MAX;

/// Fixed-capacity map from `K` to `V`; see the module documentation.
///
/// The table has at least twice as many slots as the capacity, so a probe always ends at an
/// empty slot and stays short. Removal shifts the following probe chain back instead of leaving
/// tombstones, so a long run of inserts and removals does not slow lookups down.
pub struct DenseMap<K, V> {
    entries: Vec<(K, V)>,
    table: Vec<u32>,
    capacity: u32,
}

/// The largest capacity a map can have: one index value is kept for "empty".
const fn clamped_capacity(capacity: u32) -> u32 {
    if capacity < EMPTY {
        capacity
    } else {
        EMPTY - 1
    }
}

impl<K: MapKey, V> DenseMap<K, V> {
    /// A map with room for `capacity` entries (at most `u32::MAX - 1`; more is cut to that).
    /// Like `Vec::with_capacity`, this aborts (or panics on a capacity overflow) when the
    /// system cannot provide the storage.
    #[must_use]
    pub fn with_capacity(capacity: u32) -> Self {
        let capacity = clamped_capacity(capacity);
        let slots = (u64::from(capacity) * 2).next_power_of_two().max(2);
        Self {
            entries: Vec::with_capacity(capacity as usize),
            table: vec![EMPTY; usize::try_from(slots).unwrap_or(usize::MAX)],
            capacity,
        }
    }

    /// Number of entries the map can hold at once.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity as usize
    }

    /// Number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when the map holds no entry.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Stores `value` under `key`. A key that is already there keeps its place in the order and
    /// gets the new value; the old value is returned.
    ///
    /// # Errors
    /// Returns `Err((key, value))` when the map is full and `key` is not in it.
    pub fn insert(&mut self, key: K, value: V) -> Result<Option<V>, (K, V)> {
        match self.locate(&key) {
            Ok(slot) => {
                let entry = self.entry_at(slot);
                Ok(Some(std::mem::replace(&mut self.entries[entry].1, value)))
            }
            Err(_) if self.entries.len() >= self.capacity as usize => Err((key, value)),
            Err(empty) => {
                // The count is below `capacity`, which is below `u32::MAX`.
                self.table[empty] = u32::try_from(self.entries.len()).unwrap_or(EMPTY);
                self.entries.push((key, value));
                Ok(None)
            }
        }
    }

    /// The value stored under `key`.
    #[must_use]
    pub fn get(&self, key: &K) -> Option<&V> {
        let slot = self.locate(key).ok()?;
        Some(&self.entries[self.entry_at(slot)].1)
    }

    /// The value stored under `key`, for change.
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        let slot = self.locate(key).ok()?;
        let entry = self.entry_at(slot);
        Some(&mut self.entries[entry].1)
    }

    /// True when `key` is in the map.
    #[must_use]
    pub fn contains_key(&self, key: &K) -> bool {
        self.locate(key).is_ok()
    }

    /// Removes `key` and returns its value. The last entry takes its place in the order.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let slot = self.locate(key).ok()?;
        let entry = self.entry_at(slot);
        self.close_gap(slot);
        let (_, value) = self.entries.swap_remove(entry);
        if let Some((moved, _)) = self.entries.get(entry) {
            let from = u32::try_from(self.entries.len()).unwrap_or(EMPTY);
            let at = self.slot_of_position(moved, from);
            self.table[at] = u32::try_from(entry).unwrap_or(EMPTY);
        }
        Some(value)
    }

    /// Removes every entry; the capacity stays. Costs one pass over the table, not over the entries.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.table.fill(EMPTY);
    }

    /// The entries in dense order (insertion order, changed only by removals).
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    /// The entries in dense order, values for change.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&K, &mut V)> {
        self.entries.iter_mut().map(|(k, v)| (&*k, v))
    }

    fn mask(&self) -> usize {
        self.table.len() - 1
    }

    fn home(&self, key: &K) -> usize {
        let mask = u64::try_from(self.mask()).unwrap_or(u64::MAX);
        usize::try_from(key.hash_key() & mask).unwrap_or(0)
    }

    fn entry_at(&self, slot: usize) -> usize {
        self.table[slot] as usize
    }

    fn next_slot(&self, slot: usize) -> usize {
        (slot + 1) & self.mask()
    }

    /// The table slot holding `key` (`Ok`), or the first empty slot of its probe chain (`Err`).
    fn locate(&self, key: &K) -> Result<usize, usize> {
        let mut slot = self.home(key);
        loop {
            match self.table[slot] {
                EMPTY => return Err(slot),
                position if self.entries[position as usize].0 == *key => return Ok(slot),
                _ => slot = self.next_slot(slot),
            }
        }
    }

    /// The table slot that holds the dense position `position`, which belongs to `key`.
    fn slot_of_position(&self, key: &K, position: u32) -> usize {
        let mut slot = self.home(key);
        while self.table[slot] != position {
            slot = self.next_slot(slot);
        }
        slot
    }

    /// True when the entry at `slot`, found while closing the gap at `gap`, may move back to
    /// `gap`: it is at least as far from its home slot as from the gap, counted forward and
    /// wrapping, so a lookup for it still starts at or before `gap`.
    fn may_move_back(&self, slot: usize, gap: usize) -> bool {
        let start = self.home(&self.entries[self.entry_at(slot)].0);
        let mask = self.mask();
        (slot.wrapping_sub(start) & mask) >= (slot.wrapping_sub(gap) & mask)
    }

    /// Moves the entry at `slot` back into `gap` when it may move; returns the gap that is open
    /// afterwards.
    fn pull_back(&mut self, slot: usize, gap: usize) -> usize {
        if !self.may_move_back(slot, gap) {
            return gap;
        }
        self.table[gap] = self.table[slot];
        slot
    }

    /// Empties `gap` and moves later members of its probe chain back so that no lookup stops
    /// early at the gap.
    fn close_gap(&mut self, mut gap: usize) {
        let mut slot = self.next_slot(gap);
        while self.table[slot] != EMPTY {
            gap = self.pull_back(slot, gap);
            slot = self.next_slot(slot);
        }
        self.table[gap] = EMPTY;
    }

    /// Number of table slots probed to find `key` (test aid for the hash quality).
    #[cfg(test)]
    fn probe_length(&self, key: &K) -> usize {
        let home = self.home(key);
        let end = self.locate(key).unwrap_or_else(|empty| empty);
        (end.wrapping_sub(home) & self.mask()) + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Pcg32;
    use std::cell::Cell;
    use std::rc::Rc;

    /// Unwraps a value that the test knows is there, without `expect`.
    trait Fits<T> {
        fn fits(self) -> T;
    }

    impl<T> Fits<T> for Option<T> {
        fn fits(self) -> T {
            self.unwrap_or_else(|| panic!("fits"))
        }
    }

    impl<T, E> Fits<T> for Result<T, E> {
        fn fits(self) -> T {
            self.unwrap_or_else(|_| panic!("fits"))
        }
    }

    fn order(map: &DenseMap<u32, u32>) -> Vec<(u32, u32)> {
        map.iter().map(|(k, v)| (*k, *v)).collect()
    }

    fn filled(capacity: u32, keys: &[u32]) -> DenseMap<u32, u32> {
        let mut map = DenseMap::with_capacity(capacity);
        for &k in keys {
            map.insert(k, k * 10).fits();
        }
        map
    }

    #[test]
    fn the_hash_matches_the_published_murmur3_finalizer_vectors() {
        assert_eq!(hash64(0), 0);
        assert_eq!(hash64(1), 0xb456_bcfc_34c2_cb2c);
        assert_eq!(hash64(2), 0x3abf_2a20_6506_83e7);
        assert_eq!(hash64(0xdead_beef), 0xd24b_d59f_862a_1dac);
    }

    #[test]
    fn the_hash_mixes_the_high_bits_of_the_key_into_the_low_ones() {
        // Reference values of the same finalizer, computed outside this crate.
        assert_eq!(hash64(1 << 40), 0xe80f_632e_d2f2_32f6);
        assert_eq!(hash64(u64::MAX), 0x64b5_720b_4b82_5f21);
        assert_eq!(hash64(0x8000_0000_0000_0001), 0x6dd3_9475_fbd3_dc62);
    }

    #[test]
    fn a_capacity_above_the_limit_is_cut_to_the_limit() {
        assert_eq!(clamped_capacity(5), 5);
        assert_eq!(clamped_capacity(EMPTY - 1), EMPTY - 1);
        assert_eq!(clamped_capacity(EMPTY), EMPTY - 1);
        assert_eq!(clamped_capacity(u32::MAX), EMPTY - 1);
    }

    #[test]
    fn a_map_with_an_entry_is_not_empty() {
        let map = filled(4, &[7]);
        assert!(!map.is_empty());
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn a_new_map_is_empty() {
        let map = DenseMap::<u32, u32>::with_capacity(4);
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        assert_eq!(map.capacity(), 4);
        assert_eq!(map.get(&1), None);
        assert_eq!(map.iter().count(), 0);
    }

    #[test]
    fn insert_then_get_returns_the_value() {
        let mut map = DenseMap::with_capacity(4);
        assert_eq!(map.insert(7_u32, 70_u32).fits(), None);
        assert_eq!(map.get(&7), Some(&70));
        assert!(map.contains_key(&7));
        assert_eq!(map.len(), 1);
        assert!(!map.contains_key(&8));
    }

    #[test]
    fn get_mut_changes_the_stored_value() {
        let mut map = DenseMap::with_capacity(4);
        map.insert(7_u32, 70_u32).fits();
        *map.get_mut(&7).fits() += 1;
        assert_eq!(map.get(&7), Some(&71));
        assert_eq!(map.get_mut(&8), None);
    }

    #[test]
    fn inserting_an_existing_key_replaces_the_value_and_keeps_its_place() {
        let mut map = filled(4, &[1, 2, 3]);
        assert_eq!(map.insert(2, 99).fits(), Some(20));
        assert_eq!(order(&map), vec![(1, 10), (2, 99), (3, 30)]);
        assert_eq!(map.len(), 3);
    }

    #[test]
    fn a_full_map_refuses_a_new_key_and_gives_it_back() {
        let mut map = filled(2, &[1, 2]);
        assert_eq!(map.insert(3, 30), Err((3, 30)));
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&3), None);
    }

    #[test]
    fn a_full_map_still_replaces_an_existing_key() {
        let mut map = filled(2, &[1, 2]);
        assert_eq!(map.insert(1, 5).fits(), Some(10));
        assert_eq!(map.get(&1), Some(&5));
    }

    #[test]
    fn a_map_of_capacity_zero_holds_nothing() {
        let mut map = DenseMap::<u32, u32>::with_capacity(0);
        assert_eq!(map.insert(1, 1), Err((1, 1)));
        assert_eq!(map.get(&1), None);
        assert_eq!(map.remove(&1), None);
        assert!(map.is_empty());
    }

    #[test]
    fn remove_returns_the_value_and_the_key_is_gone() {
        let mut map = filled(4, &[1, 2, 3]);
        assert_eq!(map.remove(&2), Some(20));
        assert_eq!(map.get(&2), None);
        assert!(!map.contains_key(&2));
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn removing_twice_yields_none_the_second_time() {
        let mut map = filled(4, &[1, 2, 3]);
        assert_eq!(map.remove(&2), Some(20));
        assert_eq!(map.remove(&2), None);
    }

    #[test]
    fn removing_moves_the_last_entry_into_the_hole() {
        let mut map = filled(8, &[1, 2, 3, 4, 5]);
        assert_eq!(map.remove(&2), Some(20));
        assert_eq!(order(&map), vec![(1, 10), (5, 50), (3, 30), (4, 40)]);
        assert_eq!(map.get(&5), Some(&50));
        assert_eq!(map.remove(&4), Some(40));
        assert_eq!(order(&map), vec![(1, 10), (5, 50), (3, 30)]);
        assert_eq!(map.remove(&1), Some(10));
        assert_eq!(order(&map), vec![(3, 30), (5, 50)]);
    }

    #[test]
    fn iteration_is_insertion_order_when_nothing_was_removed() {
        let keys = [9, 3, 77, 1, 40, 5];
        let map = filled(16, &keys);
        let seen: Vec<u32> = map.iter().map(|(k, _)| *k).collect();
        assert_eq!(seen, keys);
    }

    #[test]
    fn iter_mut_changes_values_in_dense_order() {
        let mut map = filled(4, &[1, 2, 3]);
        for (k, v) in map.iter_mut() {
            *v += *k;
        }
        assert_eq!(order(&map), vec![(1, 11), (2, 22), (3, 33)]);
    }

    #[test]
    fn clear_removes_everything_and_the_map_is_usable_again() {
        let mut map = filled(3, &[1, 2, 3]);
        map.clear();
        assert!(map.is_empty());
        assert_eq!(map.get(&1), None);
        assert_eq!(map.capacity(), 3);
        for k in [4, 5, 6] {
            map.insert(k, k).fits();
        }
        assert_eq!(map.insert(7, 7), Err((7, 7)));
        assert_eq!(order(&map), vec![(4, 4), (5, 5), (6, 6)]);
    }

    #[test]
    fn handles_and_u64_keys_work() {
        let mut ints = DenseMap::with_capacity(4);
        ints.insert(1_u64 << 40, 1_u8).fits();
        assert_eq!(ints.get(&(1_u64 << 40)), Some(&1));
        let mut pool = crate::pool::Pool::<u8>::with_capacity(2);
        let (a, b) = (pool.insert(0).fits(), pool.insert(0).fits());
        let mut by_handle = DenseMap::with_capacity(4);
        by_handle.insert(a, "a").fits();
        by_handle.insert(b, "b").fits();
        assert_eq!(by_handle.get(&a), Some(&"a"));
        assert_eq!(by_handle.get(&b), Some(&"b"));
    }

    #[test]
    fn key_hashes_are_the_finalizer_of_the_widened_key() {
        assert_eq!(7_u32.hash_key(), hash64(7));
        assert_eq!(u32::MAX.hash_key(), hash64(u64::from(u32::MAX)));
        assert_eq!((1_u64 << 40).hash_key(), hash64(1 << 40));
        let mut pool = crate::pool::Pool::<u8>::with_capacity(1);
        let old = pool.insert(0).fits();
        pool.remove(old);
        let new = pool.insert(0).fits();
        let packed = |h: Handle<u8>| u64::from(h.index()) | (u64::from(h.generation()) << 32);
        assert_eq!(old.hash_key(), hash64(packed(old)));
        assert_eq!(new.hash_key(), hash64(packed(new)));
        assert_ne!(old.hash_key(), new.hash_key());
    }

    #[test]
    fn a_stale_handle_is_a_different_key_from_the_new_one_in_its_slot() {
        let mut pool = crate::pool::Pool::<u8>::with_capacity(1);
        let old = pool.insert(0).fits();
        pool.remove(old);
        let new = pool.insert(0).fits();
        let mut map = DenseMap::with_capacity(2);
        map.insert(old, 1_u8).fits();
        assert_eq!(map.get(&new), None);
        assert_eq!(map.get(&old), Some(&1));
    }

    fn random_step(map: &mut DenseMap<u32, u32>, rng: &mut Pcg32) {
        let key = rng.next_u32() % 40;
        if rng.next_u32().is_multiple_of(3) {
            map.remove(&key);
        } else {
            map.insert(key, key).fits();
        }
    }

    fn random_order(capacity: u32) -> Vec<u32> {
        let mut map = DenseMap::with_capacity(capacity);
        let mut rng = Pcg32::new(7, 3);
        for _ in 0..400 {
            random_step(&mut map, &mut rng);
        }
        map.iter().map(|(k, _)| *k).collect()
    }

    #[test]
    fn the_order_does_not_depend_on_the_capacity() {
        let small = random_order(40);
        assert_eq!(small, random_order(64));
        assert_eq!(small, random_order(1000));
    }

    const MODEL_CAPACITY: usize = 48;

    /// Inserts into the map and into the vector model, which is the definition of the order.
    fn insert_both(
        map: &mut DenseMap<u32, u32>,
        model: &mut Vec<(u32, u32)>,
        key: u32,
        value: u32,
    ) {
        let at = model.iter().position(|(k, _)| *k == key);
        match (at, model.len() < MODEL_CAPACITY) {
            (Some(i), _) => {
                let old = std::mem::replace(&mut model[i].1, value);
                assert_eq!(map.insert(key, value).fits(), Some(old));
            }
            (None, true) => {
                assert_eq!(map.insert(key, value).fits(), None);
                model.push((key, value));
            }
            (None, false) => assert_eq!(map.insert(key, value), Err((key, value))),
        }
    }

    fn remove_both(map: &mut DenseMap<u32, u32>, model: &mut Vec<(u32, u32)>, key: u32) {
        let expected = model
            .iter()
            .position(|(k, _)| *k == key)
            .map(|i| model.swap_remove(i).1);
        assert_eq!(map.remove(&key), expected);
    }

    fn agree(map: &DenseMap<u32, u32>, model: &[(u32, u32)]) {
        assert_eq!(order(map), model);
        for probe in 0..64 {
            assert_eq!(
                map.get(&probe),
                model.iter().find(|(k, _)| *k == probe).map(|(_, v)| v)
            );
        }
    }

    fn agree_every_97th(step: u32, map: &DenseMap<u32, u32>, model: &[(u32, u32)]) {
        if step.is_multiple_of(97) {
            agree(map, model);
        }
    }

    #[test]
    fn random_operations_agree_with_a_vector_model() {
        let mut rng = Pcg32::new(11, 5);
        let mut map = DenseMap::with_capacity(48);
        let mut model: Vec<(u32, u32)> = Vec::new();
        for step in 0..20_000_u32 {
            let key = rng.next_u32() % 64;
            match rng.next_u32() % 3 {
                0 => remove_both(&mut map, &mut model, key),
                _ => insert_both(&mut map, &mut model, key, step),
            }
            assert_eq!(map.len(), model.len());
            agree_every_97th(step, &map, &model);
        }
        agree(&map, &model);
    }

    #[test]
    fn keys_that_differ_only_in_the_high_bits_do_not_pile_up() {
        let mut map = DenseMap::with_capacity(10_000);
        for i in 0..10_000_u64 {
            map.insert(i << 32, i).fits();
        }
        let longest = (0..10_000_u64)
            .map(|i| map.probe_length(&(i << 32)))
            .max()
            .fits();
        assert!(longest <= 16, "longest probe {longest}");
        let total: usize = (0..10_000_u64).map(|i| map.probe_length(&(i << 32))).sum();
        assert!(total <= 15_000, "mean probe above 1.5: {total}");
    }

    fn strided_map(stride: u64) -> DenseMap<u64, u64> {
        let mut map = DenseMap::with_capacity(4096);
        for i in 0..4096_u64 {
            map.insert(i * stride, i).fits();
        }
        map
    }

    #[test]
    fn counter_keys_and_multiples_of_a_power_of_two_do_not_pile_up() {
        for stride in [1_u64, 1 << 8, 1 << 16, 1 << 32] {
            let map = strided_map(stride);
            let longest = (0..4096_u64)
                .map(|i| map.probe_length(&(i * stride)))
                .max()
                .fits();
            assert!(longest <= 40, "stride {stride}: longest probe {longest}");
        }
    }

    struct Counted(Rc<Cell<u32>>);

    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn values_are_dropped_once_by_remove_clear_replace_and_drop() {
        let drops = Rc::new(Cell::new(0));
        let mut map = DenseMap::with_capacity(8);
        for k in 0..5_u32 {
            map.insert(k, Counted(Rc::clone(&drops))).fits();
        }
        drop(map.remove(&0));
        assert_eq!(drops.get(), 1);
        drop(map.insert(1, Counted(Rc::clone(&drops))).fits());
        assert_eq!(drops.get(), 2);
        map.clear();
        assert_eq!(drops.get(), 6);
        map.insert(9, Counted(Rc::clone(&drops))).fits();
        drop(map);
        assert_eq!(drops.get(), 7);
    }
}
