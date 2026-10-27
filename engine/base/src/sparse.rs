//! A fixed-capacity sparse set: values stored densely, found from a small integer id.

/// Why an insert was refused; the value is handed back.
#[derive(Debug, PartialEq, Eq)]
pub enum InsertError<V> {
    /// The id is not below the id bound of the set.
    IdOutOfBound(V),
    /// The set holds `capacity` entries and the id is not one of them.
    Full(V),
}

/// Fixed-capacity map from a small integer id (below the id bound) to `V`.
///
/// Entries live in one dense array in the order they were inserted; removing an entry moves the
/// last entry into the hole (swap-remove), so the iteration order is a function of the sequence of
/// operations alone: for the same accepted operations it is the same whatever the id bound or
/// capacity, the process, the thread count or the platform (which operations are accepted does
/// depend on the bound and the capacity). A table with one `u32` position per possible id finds an entry from its
/// id; an entry is present only if the dense array confirms it, so the table is never cleared and
/// `clear` does not touch it. Memory is `4 * id_bound` bytes of table plus `size_of::<(u32, V)>()`
/// per unit of capacity. Both arrays are allocated when the set is built; insert, remove and
/// lookup make no allocator call afterwards. Failure to fit is a value.
pub struct SparseSet<V> {
    entries: Vec<(u32, V)>,
    positions: Vec<u32>,
    capacity: u32,
}

impl<V> SparseSet<V> {
    /// A set for ids below `id_bound` that holds at most `capacity` entries at once (`capacity` is cut
    /// to `id_bound`, which is the most ids there can be). Like
    /// `Vec::with_capacity`, this aborts when the system cannot provide the storage.
    #[must_use]
    pub fn with_capacity(id_bound: u32, capacity: u32) -> Self {
        let capacity = capacity.min(id_bound);
        Self {
            entries: Vec::with_capacity(capacity as usize),
            positions: vec![0; id_bound as usize],
            capacity,
        }
    }

    /// Ids must be below this bound.
    #[must_use]
    pub fn id_bound(&self) -> u32 {
        u32::try_from(self.positions.len()).unwrap_or(u32::MAX)
    }

    /// Number of entries the set can hold at once.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity as usize
    }

    /// Number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when the set holds no entry.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Stores `value` under `id`. An id that is already there keeps its place in the order and
    /// gets the new value; the old value is returned.
    ///
    /// # Errors
    /// Returns the value back when `id` is not below the id bound, or when the set is full and
    /// `id` is not in it.
    pub fn insert(&mut self, id: u32, value: V) -> Result<Option<V>, InsertError<V>> {
        if let Some(position) = self.position_of(id) {
            return Ok(Some(std::mem::replace(
                &mut self.entries[position].1,
                value,
            )));
        }
        let Some(slot) = self.positions.get_mut(id as usize) else {
            return Err(InsertError::IdOutOfBound(value));
        };
        if self.entries.len() >= self.capacity as usize {
            return Err(InsertError::Full(value));
        }
        // The count is below `capacity`, which fits a `u32`.
        *slot = u32::try_from(self.entries.len()).unwrap_or(0);
        self.entries.push((id, value));
        Ok(None)
    }

    /// The value stored under `id`.
    #[must_use]
    pub fn get(&self, id: u32) -> Option<&V> {
        Some(&self.entries[self.position_of(id)?].1)
    }

    /// The value stored under `id`, for change.
    pub fn get_mut(&mut self, id: u32) -> Option<&mut V> {
        let position = self.position_of(id)?;
        Some(&mut self.entries[position].1)
    }

    /// True when `id` is in the set.
    #[must_use]
    pub fn contains(&self, id: u32) -> bool {
        self.position_of(id).is_some()
    }

    /// Removes `id` and returns its value. The last entry takes its place in the order.
    pub fn remove(&mut self, id: u32) -> Option<V> {
        let position = self.position_of(id)?;
        let (_, value) = self.entries.swap_remove(position);
        if let Some((moved, _)) = self.entries.get(position) {
            self.positions[*moved as usize] = u32::try_from(position).unwrap_or(0);
        }
        Some(value)
    }

    /// Removes every entry; the limits stay. Costs one pass over the entries (to drop them), none
    /// over the table.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// The entries in dense order (insertion order, changed only by removals).
    pub fn iter(&self) -> impl Iterator<Item = (u32, &V)> {
        self.entries.iter().map(|(id, v)| (*id, v))
    }

    /// The entries in dense order, values for change.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (u32, &mut V)> {
        self.entries.iter_mut().map(|(id, v)| (*id, v))
    }

    /// The dense position of `id`, if the id is in the set: the table says where to look and the
    /// dense array confirms it (a stale table entry names another id or lies past the end).
    fn position_of(&self, id: u32) -> Option<usize> {
        let position = *self.positions.get(id as usize)? as usize;
        (self.entries.get(position)?.0 == id).then_some(position)
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

    fn order(set: &SparseSet<u32>) -> Vec<(u32, u32)> {
        set.iter().map(|(id, v)| (id, *v)).collect()
    }

    fn filled(id_bound: u32, capacity: u32, ids: &[u32]) -> SparseSet<u32> {
        let mut set = SparseSet::with_capacity(id_bound, capacity);
        for &id in ids {
            set.insert(id, id * 10).fits();
        }
        set
    }

    #[test]
    fn a_new_set_is_empty_and_reports_its_limits() {
        let set = SparseSet::<u32>::with_capacity(100, 8);
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
        assert_eq!(set.id_bound(), 100);
        assert_eq!(set.capacity(), 8);
        assert_eq!(set.get(5), None);
        assert!(!set.contains(5));
    }

    #[test]
    fn insert_then_get_and_contains() {
        let set = filled(100, 8, &[5, 7]);
        assert_eq!(set.len(), 2);
        assert_eq!(set.get(5), Some(&50));
        assert_eq!(set.get(7), Some(&70));
        assert!(set.contains(7));
        assert!(!set.contains(6));
    }

    #[test]
    fn get_mut_changes_the_value() {
        let mut set = filled(100, 8, &[5]);
        *set.get_mut(5).fits() += 1;
        assert_eq!(set.get(5), Some(&51));
        assert_eq!(set.get_mut(6), None);
    }

    #[test]
    fn inserting_an_existing_id_replaces_the_value_and_keeps_the_place() {
        let mut set = filled(100, 8, &[5, 7, 9]);
        assert_eq!(set.insert(7, 1).fits(), Some(70));
        assert_eq!(order(&set), vec![(5, 50), (7, 1), (9, 90)]);
    }

    #[test]
    fn an_id_at_or_above_the_bound_is_refused_and_the_value_comes_back() {
        let mut set = SparseSet::with_capacity(10, 8);
        assert_eq!(set.insert(10, 1_u8), Err(InsertError::IdOutOfBound(1)));
        assert_eq!(
            set.insert(u32::MAX, 2_u8),
            Err(InsertError::IdOutOfBound(2))
        );
        assert_eq!(set.insert(9, 3_u8), Ok(None));
        assert_eq!(set.get(10), None);
        assert_eq!(set.get(u32::MAX), None);
        assert!(!set.contains(u32::MAX));
        assert_eq!(set.remove(u32::MAX), None);
    }

    #[test]
    fn an_out_of_bound_id_is_reported_before_a_full_set() {
        let mut none = SparseSet::<u8>::with_capacity(10, 0);
        assert_eq!(none.insert(10, 1), Err(InsertError::IdOutOfBound(1)));
        let mut full = filled(100, 2, &[5, 7]);
        assert_eq!(full.insert(100, 1), Err(InsertError::IdOutOfBound(1)));
    }

    #[test]
    fn the_capacity_is_cut_to_the_id_bound() {
        let mut set = SparseSet::<u8>::with_capacity(3, 10);
        assert_eq!(set.capacity(), 3);
        for id in 0..3 {
            set.insert(id, 0).fits();
        }
        assert_eq!(set.insert(3, 0), Err(InsertError::IdOutOfBound(0)));
    }

    #[test]
    fn a_full_set_refuses_a_new_id_but_still_replaces() {
        let mut set = filled(100, 2, &[5, 7]);
        assert_eq!(set.insert(9, 1), Err(InsertError::Full(1)));
        assert_eq!(set.insert(7, 2), Ok(Some(70)));
        assert_eq!(order(&set), vec![(5, 50), (7, 2)]);
    }

    #[test]
    fn a_set_with_no_capacity_or_no_ids_refuses_everything() {
        let mut none = SparseSet::<u8>::with_capacity(10, 0);
        assert_eq!(none.insert(1, 1), Err(InsertError::Full(1)));
        let mut empty_universe = SparseSet::<u8>::with_capacity(0, 4);
        assert_eq!(
            empty_universe.insert(0, 1),
            Err(InsertError::IdOutOfBound(1))
        );
    }

    #[test]
    fn remove_returns_the_value_and_forgets_the_id() {
        let mut set = filled(100, 8, &[5, 7]);
        assert_eq!(set.remove(5), Some(50));
        assert_eq!(set.remove(5), None);
        assert!(!set.contains(5));
        assert_eq!(set.get(7), Some(&70));
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn remove_moves_the_last_entry_into_the_hole() {
        let mut set = filled(100, 8, &[1, 2, 3, 4, 5]);
        set.remove(2).fits();
        assert_eq!(order(&set), vec![(1, 10), (5, 50), (3, 30), (4, 40)]);
        assert_eq!(set.get(5), Some(&50));
        set.remove(4).fits();
        assert_eq!(order(&set), vec![(1, 10), (5, 50), (3, 30)]);
    }

    #[test]
    fn a_removed_id_can_be_inserted_again_at_the_end() {
        let mut set = filled(100, 8, &[1, 2, 3]);
        set.remove(1).fits();
        set.insert(1, 11).fits();
        assert_eq!(order(&set), vec![(3, 30), (2, 20), (1, 11)]);
    }

    #[test]
    fn iteration_is_in_insertion_order_and_iter_mut_changes_values() {
        let mut set = filled(100, 8, &[9, 3, 7]);
        assert_eq!(order(&set), vec![(9, 90), (3, 30), (7, 70)]);
        for (_, v) in set.iter_mut() {
            *v += 1;
        }
        assert_eq!(order(&set), vec![(9, 91), (3, 31), (7, 71)]);
    }

    #[test]
    fn clear_forgets_every_id_and_keeps_the_limits() {
        let mut set = filled(100, 8, &[5, 7]);
        set.clear();
        assert!(set.is_empty());
        assert!(!set.contains(5));
        assert_eq!(set.get(7), None);
        assert_eq!(set.id_bound(), 100);
        assert_eq!(set.capacity(), 8);
        set.insert(7, 1).fits();
        assert_eq!(order(&set), vec![(7, 1)]);
    }

    #[test]
    fn stale_table_entries_after_clear_do_not_make_ids_appear() {
        let mut set = filled(100, 8, &[5, 7, 9]);
        set.clear();
        set.insert(9, 1).fits();
        assert!(!set.contains(5));
        assert!(!set.contains(7));
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn the_order_does_not_depend_on_the_id_bound_or_capacity() {
        let ops = [1_u32, 2, 3, 4, 5, 6];
        let run = |bound, cap| {
            let mut set = filled(bound, cap, &ops);
            set.remove(2).fits();
            set.remove(6).fits();
            set.insert(2, 1).fits();
            order(&set)
        };
        assert_eq!(run(10, 6), run(1_000_000, 100));
    }

    struct Counted(Rc<Cell<u32>>);

    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn values_are_dropped_exactly_once() {
        let drops = Rc::new(Cell::new(0));
        let mut set = SparseSet::with_capacity(10, 4);
        for id in 0..4 {
            set.insert(id, Counted(Rc::clone(&drops))).fits();
        }
        drop(set.remove(1));
        assert_eq!(drops.get(), 1);
        set.clear();
        assert_eq!(drops.get(), 4);
        set.insert(0, Counted(Rc::clone(&drops))).fits();
        drop(set);
        assert_eq!(drops.get(), 5);
    }

    fn random_step(set: &mut SparseSet<u32>, model: &mut Vec<(u32, u32)>, rng: &mut Pcg32) {
        let id = rng.next_u32() % 40;
        let value = rng.next_u32();
        match rng.next_u32() % 3 {
            0 => step_insert(set, model, id, value),
            1 => step_remove(set, model, id),
            _ => assert_eq!(set.get(id).copied(), find(model, id)),
        }
    }

    fn find(model: &[(u32, u32)], id: u32) -> Option<u32> {
        model.iter().find(|e| e.0 == id).map(|e| e.1)
    }

    fn step_insert(set: &mut SparseSet<u32>, model: &mut Vec<(u32, u32)>, id: u32, value: u32) {
        let at = model.iter().position(|e| e.0 == id);
        match (at, model.len() >= 24) {
            (Some(p), _) => {
                let old = std::mem::replace(&mut model[p].1, value);
                assert_eq!(set.insert(id, value), Ok(Some(old)));
            }
            (None, true) => assert_eq!(set.insert(id, value), Err(InsertError::Full(value))),
            (None, false) => {
                model.push((id, value));
                assert_eq!(set.insert(id, value), Ok(None));
            }
        }
    }

    fn step_remove(set: &mut SparseSet<u32>, model: &mut Vec<(u32, u32)>, id: u32) {
        let expected = model
            .iter()
            .position(|e| e.0 == id)
            .map(|p| model.swap_remove(p).1);
        assert_eq!(set.remove(id), expected);
    }

    #[test]
    fn a_long_random_run_agrees_with_a_vector_that_swap_removes() {
        let mut set = SparseSet::with_capacity(40, 24);
        let mut model = Vec::new();
        let mut rng = Pcg32::new(7, 3);
        for _ in 0..20_000 {
            random_step(&mut set, &mut model, &mut rng);
            assert_eq!(order(&set), model);
        }
    }
}
