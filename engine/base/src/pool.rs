//! Generational handles and a fixed-capacity pool of long-lived objects.
//!
//! A [`Handle`] names an object by slot index and generation instead of by
//! pointer, so other modules can keep it without borrowing the pool and a stale
//! handle (its object was removed) is detected instead of reading a new object.
//! The [`Pool`] allocates its storage once, when it is built; insert, remove and
//! lookup make no allocator call afterwards. Failure to fit is a value.

use core::fmt;
use core::hash::{Hash, Hasher};
use core::marker::PhantomData;

/// Names one object in a [`Pool<T>`]: the slot index and the generation that
/// slot had when the object was inserted. `Copy` for every `T`. A handle of one
/// element type cannot be given to a pool of another:
///
/// ```compile_fail,E0308
/// use base::pool::Pool;
/// let mut ints = Pool::<u32>::with_capacity(1);
/// let strings = Pool::<String>::with_capacity(1);
/// let handle = ints.insert(1).unwrap();
/// let _ = strings.get(handle);
/// ```
pub struct Handle<T> {
    index: u32,
    generation: u32,
    marker: PhantomData<fn() -> T>,
}

impl<T> Handle<T> {
    fn new(index: u32, generation: u32) -> Self {
        Self {
            index,
            generation,
            marker: PhantomData,
        }
    }

    /// Rebuilds a handle from the two numbers [`Handle::index`] and
    /// [`Handle::generation`] returned, for code that stores handles as plain
    /// bytes (a command stream); use it for decoding only. It grants nothing: a pool still checks the
    /// generation, so a forged or stale handle is refused like any other.
    #[must_use]
    pub fn from_raw(index: u32, generation: u32) -> Self {
        Self::new(index, generation)
    }

    /// Slot index inside the pool.
    #[must_use]
    pub fn index(self) -> u32 {
        self.index
    }

    /// Number of times the slot was freed before this handle was issued.
    #[must_use]
    pub fn generation(self) -> u32 {
        self.generation
    }
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Handle<T> {}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

impl<T> Eq for Handle<T> {}

impl<T> Hash for Handle<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.index.hash(state);
        self.generation.hash(state);
    }
}

impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Handle({}v{})", self.index, self.generation)
    }
}

struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

/// Fixed-capacity pool addressed by [`Handle`]s.
///
/// Slots are issued in index order and reused last freed first, so the handles
/// a sequence of operations produces are the same on every run. A slot whose
/// generation can no longer advance is retired, never reused: a stale handle is
/// always detected, at the price of one slot of capacity per 2^32 reuses.
pub struct Pool<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
    capacity: u32,
    len: usize,
}

impl<T> Pool<T> {
    /// A pool with room for `capacity` objects. Like `Vec::with_capacity`, this
    /// aborts (or panics on a capacity overflow) when the system cannot provide
    /// the storage.
    #[must_use]
    pub fn with_capacity(capacity: u32) -> Self {
        let room = capacity as usize;
        Self {
            slots: Vec::with_capacity(room),
            free: Vec::with_capacity(room),
            capacity,
            len: 0,
        }
    }

    /// Number of objects the pool can hold at once.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity as usize
    }

    /// Number of live objects.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// True when the pool holds no live object.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Stores `value` and returns its handle, or gives `value` back when the
    /// pool is full.
    ///
    /// # Errors
    /// Returns `Err(value)` when every slot is in use or retired.
    pub fn insert(&mut self, value: T) -> Result<Handle<T>, T> {
        let Some(index) = self.free.pop().or_else(|| self.open_new_slot()) else {
            return Err(value);
        };
        let Some(slot) = self.slots.get_mut(index as usize) else {
            // Unreachable: free indices always name an opened slot. Keep the index.
            self.free.push(index);
            return Err(value);
        };
        slot.value = Some(value);
        self.len += 1;
        Ok(Handle::new(index, slot.generation))
    }

    /// Takes the object out of the pool; `None` for a stale handle. A handle
    /// issued by another pool of the same `T` is not detected.
    pub fn remove(&mut self, handle: Handle<T>) -> Option<T> {
        let slot = self.slots.get_mut(handle.index as usize)?;
        if slot.generation != handle.generation {
            return None;
        }
        let value = slot.value.take()?;
        self.len -= 1;
        if let Some(next) = slot.generation.checked_add(1) {
            slot.generation = next;
            self.free.push(handle.index);
        }
        Some(value)
    }

    /// The object `handle` names, or `None` when the handle is stale.
    #[must_use]
    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        let slot = self.slots.get(handle.index as usize)?;
        if slot.generation == handle.generation {
            slot.value.as_ref()
        } else {
            None
        }
    }

    /// Mutable access to the object `handle` names, or `None` when stale.
    #[must_use]
    pub fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> {
        let slot = self.slots.get_mut(handle.index as usize)?;
        if slot.generation == handle.generation {
            slot.value.as_mut()
        } else {
            None
        }
    }

    /// True when `handle` names a live object of this pool.
    #[must_use]
    pub fn contains(&self, handle: Handle<T>) -> bool {
        self.get(handle).is_some()
    }

    /// Index of a slot never used before, or `None` when all of them were.
    fn open_new_slot(&mut self) -> Option<u32> {
        if self.slots.len() >= self.capacity as usize {
            return None;
        }
        let index = u32::try_from(self.slots.len()).ok()?;
        self.slots.push(Slot {
            generation: 0,
            value: None,
        });
        Some(index)
    }
}

impl<T> fmt::Debug for Pool<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pool")
            .field("len", &self.len)
            .field("capacity", &self.capacity())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Pcg32;
    use std::cell::Cell;
    use std::collections::HashSet;
    use std::collections::hash_map::DefaultHasher;
    use std::rc::Rc;

    #[test]
    fn from_raw_round_trips_and_a_forged_handle_is_rejected() {
        let mut pool = Pool::<u32>::with_capacity(2);
        let real = pool.insert(7).fits();
        let rebuilt = Handle::<u32>::from_raw(real.index(), real.generation());
        assert_eq!(rebuilt, real);
        assert_eq!(pool.get(rebuilt), Some(&7));
        let forged = Handle::<u32>::from_raw(real.index(), real.generation() + 1);
        assert_eq!(pool.get(forged), None);
        let out_of_range = Handle::<u32>::from_raw(99, 0);
        assert_eq!(pool.get(out_of_range), None);
    }

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

    struct Counted(Rc<Cell<u32>>);

    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    fn filled(capacity: u32) -> (Pool<u32>, Vec<Handle<u32>>) {
        let mut pool = Pool::with_capacity(capacity);
        let handles = (0..capacity).map(|v| pool.insert(v).fits()).collect();
        (pool, handles)
    }

    #[test]
    fn insert_then_get_returns_the_value() {
        let mut pool = Pool::with_capacity(4);
        let h = pool.insert(7_u32).fits();
        assert_eq!(pool.get(h), Some(&7));
        assert!(pool.contains(h));
    }

    #[test]
    fn get_mut_changes_the_stored_value() {
        let mut pool = Pool::with_capacity(4);
        let h = pool.insert(7_u32).fits();
        *pool.get_mut(h).fits() += 1;
        assert_eq!(pool.get(h), Some(&8));
    }

    #[test]
    fn remove_returns_the_value_and_the_handle_goes_stale() {
        let mut pool = Pool::with_capacity(4);
        let h = pool.insert(7_u32).fits();
        assert_eq!(pool.remove(h), Some(7));
        assert_eq!(pool.get(h), None);
        assert!(!pool.contains(h));
        assert_eq!(pool.get_mut(h), None);
    }

    #[test]
    fn removing_twice_yields_none_the_second_time() {
        let mut pool = Pool::with_capacity(4);
        let h = pool.insert(7_u32).fits();
        assert_eq!(pool.remove(h), Some(7));
        assert_eq!(pool.remove(h), None);
    }

    #[test]
    fn a_reused_slot_gets_a_new_generation_and_the_old_handle_stays_stale() {
        let mut pool = Pool::with_capacity(1);
        let old = pool.insert(1_u32).fits();
        pool.remove(old);
        let new = pool.insert(2).fits();
        assert_eq!(new.index(), old.index());
        assert_eq!(new.generation(), old.generation() + 1);
        assert_ne!(new, old);
        assert_eq!(pool.get(old), None);
        assert_eq!(pool.get(new), Some(&2));
        assert_eq!(pool.remove(old), None);
        assert_eq!(pool.get(new), Some(&2));
    }

    #[test]
    fn a_full_pool_gives_the_value_back_and_a_removal_makes_room() {
        let (mut pool, handles) = filled(3);
        assert_eq!(pool.insert(99), Err(99));
        assert_eq!(pool.len(), 3);
        pool.remove(handles[1]);
        assert!(pool.insert(99).is_ok());
        assert_eq!(pool.insert(100), Err(100));
    }

    #[test]
    fn a_zero_capacity_pool_holds_nothing() {
        let mut pool = Pool::with_capacity(0);
        assert_eq!(pool.insert(1_u32), Err(1));
        assert_eq!(pool.capacity(), 0);
        assert!(pool.is_empty());
    }

    #[test]
    fn len_capacity_and_is_empty_track_the_contents() {
        let mut pool = Pool::with_capacity(5);
        assert_eq!((pool.len(), pool.capacity(), pool.is_empty()), (0, 5, true));
        let a = pool.insert(1_u32).fits();
        let b = pool.insert(2).fits();
        assert_eq!((pool.len(), pool.is_empty()), (2, false));
        pool.remove(a);
        assert_eq!(pool.len(), 1);
        pool.remove(b);
        assert_eq!((pool.len(), pool.is_empty()), (0, true));
        assert_eq!(pool.capacity(), 5);
    }

    #[test]
    fn slots_are_reused_last_freed_first_so_handles_are_deterministic() {
        let (mut pool, handles) = filled(3);
        pool.remove(handles[0]);
        pool.remove(handles[2]);
        let first = pool.insert(10).fits();
        let second = pool.insert(11).fits();
        assert_eq!(first.index(), handles[2].index());
        assert_eq!(second.index(), handles[0].index());
    }

    #[test]
    fn fresh_slots_are_issued_in_index_order() {
        let (_, handles) = filled(4);
        let indices: Vec<u32> = handles.iter().map(|h| h.index()).collect();
        assert_eq!(indices, [0, 1, 2, 3]);
        assert!(handles.iter().all(|h| h.generation() == 0));
    }

    #[test]
    fn a_handle_beyond_the_used_slots_is_stale() {
        let mut pool = Pool::<u32>::with_capacity(4);
        let (other, handles) = filled(4);
        drop(other);
        assert_eq!(pool.get(handles[3]), None);
        assert_eq!(pool.remove(handles[3]), None);
    }

    #[test]
    fn a_slot_whose_generation_cannot_advance_is_retired() {
        let mut pool = Pool::with_capacity(2);
        let a = pool.insert(1_u32).fits();
        pool.slots[a.index() as usize].generation = u32::MAX;
        let worn = Handle::new(a.index(), u32::MAX);
        assert_eq!(pool.remove(a), None, "old generation is stale already");
        assert_eq!(pool.remove(worn), Some(1));
        assert_eq!(pool.get(worn), None);
        let b = pool.insert(2).fits();
        assert_ne!(b.index(), a.index());
        assert_eq!(pool.insert(3), Err(3), "the retired slot is never reused");
        assert_eq!(pool.get(worn), None);
    }

    #[test]
    fn handles_are_copy_and_comparable_for_any_type() {
        let mut pool: Pool<String> = Pool::with_capacity(2);
        let h = pool.insert(String::from("x")).fits();
        let copy = h;
        assert_eq!(h, copy);
        let set: HashSet<Handle<String>> = [h, copy].into_iter().collect();
        assert_eq!(set.len(), 1);
        assert_eq!(format!("{h:?}"), "Handle(0v0)");
    }

    #[test]
    fn retiring_a_slot_lowers_len_but_keeps_capacity_and_other_free_slots() {
        let (mut pool, handles) = filled(3);
        pool.remove(handles[0]);
        pool.slots[handles[1].index() as usize].generation = u32::MAX;
        let worn = Handle::new(handles[1].index(), u32::MAX);
        assert_eq!(pool.remove(worn), Some(1));
        assert_eq!((pool.len(), pool.capacity()), (1, 3));
        let again = pool.insert(7).fits();
        assert_eq!(again.index(), handles[0].index());
        assert_eq!(
            pool.insert(8),
            Err(8),
            "slot 1 is retired, slots 0 and 2 are full"
        );
    }

    #[test]
    fn get_mut_with_an_old_handle_is_none_after_the_slot_is_reused() {
        let mut pool = Pool::with_capacity(1);
        let old = pool.insert(1_u32).fits();
        pool.remove(old);
        let new = pool.insert(2).fits();
        assert_eq!(pool.get_mut(old), None);
        assert_eq!(pool.get_mut(new), Some(&mut 2));
    }

    #[test]
    fn zero_sized_values_are_stored_and_removed() {
        let mut pool = Pool::with_capacity(2);
        let a = pool.insert(()).fits();
        let b = pool.insert(()).fits();
        assert_eq!(pool.insert(()), Err(()));
        assert_eq!(pool.remove(a), Some(()));
        assert!(pool.contains(b) && !pool.contains(a));
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn equal_handles_hash_alike_and_different_handles_differ() {
        fn hash_of(handle: Handle<u32>) -> u64 {
            let mut hasher = DefaultHasher::new();
            handle.hash(&mut hasher);
            hasher.finish()
        }
        let (_, handles) = filled(2);
        assert_eq!(hash_of(handles[0]), hash_of(handles[0]));
        assert_ne!(hash_of(handles[0]), hash_of(handles[1]));
        assert_ne!(hash_of(handles[0]), hash_of(Handle::new(0, 1)));
    }

    #[test]
    fn pool_debug_shows_length_and_capacity() {
        let (pool, _) = filled(3);
        assert_eq!(format!("{pool:?}"), "Pool { len: 3, capacity: 3, .. }");
    }

    #[test]
    fn values_with_drop_glue_are_dropped_with_the_pool_or_returned_by_remove() {
        let drops = Rc::new(Cell::new(0_u32));
        let mut pool = Pool::with_capacity(3);
        let a = pool.insert(Counted(drops.clone())).ok().fits();
        pool.insert(Counted(drops.clone())).ok().fits();
        drop(pool.remove(a));
        assert_eq!(drops.get(), 1);
        drop(pool);
        assert_eq!(drops.get(), 2);
    }

    const CAPACITY: u32 = 16;

    /// A pool next to a plain list of what it should hold.
    struct Model {
        pool: Pool<u32>,
        live: Vec<(Handle<u32>, u32)>,
        dead: Vec<Handle<u32>>,
    }

    fn model_insert(model: &mut Model, value: u32) {
        match model.pool.insert(value) {
            Ok(handle) => model.live.push((handle, value)),
            Err(back) => assert_eq!((back, model.live.len()), (value, CAPACITY as usize)),
        }
    }

    fn model_remove(model: &mut Model, pick: usize) {
        let (handle, value) = model.live.swap_remove(pick);
        assert_eq!(model.pool.remove(handle), Some(value));
        model.dead.push(handle);
    }

    fn model_step(model: &mut Model, rng: &mut Pcg32, value: u32) {
        let action = rng.below(3).fits();
        let live = u32::try_from(model.live.len()).fits();
        if action == 0 {
            model_insert(model, value);
        } else if action == 1 && live > 0 {
            model_remove(model, rng.below(live).fits() as usize);
        }
    }

    fn model_check(model: &Model) {
        assert_eq!(model.pool.len(), model.live.len());
        for (handle, value) in &model.live {
            assert_eq!(model.pool.get(*handle), Some(value));
        }
        for handle in &model.dead {
            assert_eq!(model.pool.get(*handle), None);
        }
    }

    #[test]
    fn pool_matches_a_simple_model_over_random_operations() {
        const OPERATIONS: u32 = 20_000;
        let mut rng = Pcg32::new(0x1234, 9);
        let mut model = Model {
            pool: Pool::with_capacity(CAPACITY),
            live: Vec::new(),
            dead: Vec::new(),
        };
        for step in 0..OPERATIONS {
            model_step(&mut model, &mut rng, step);
            model_check(&model);
        }
    }
}
