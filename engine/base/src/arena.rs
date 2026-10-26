//! Fixed-capacity bump arena for per-frame and per-job scratch data.
//!
//! One block of memory is obtained when the arena is built; after that every
//! allocation is a pointer bump and makes no allocator call. Only types without
//! drop glue can be stored, so `reset` has nothing to run. Failure to fit is a
//! value (`None`, or `Err(value)` from [`ArenaVec::push`]), never a panic.
#![expect(
    unsafe_code,
    reason = "the arena hands out memory from one raw block; every unsafe block has a SAFETY comment"
)]

use core::cell::Cell;
use core::fmt;
use core::marker::PhantomData;
use core::mem::{align_of, needs_drop, size_of};
use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;
use std::alloc::{Layout, handle_alloc_error};

/// Alignment of the backing block, a cache line. Padding is computed from the
/// offset, so whether an allocation fits never depends on where the allocator
/// put the block. Types with a larger alignment are rejected at compile time.
const BLOCK_ALIGN: usize = 64;

/// Evaluated at compile time for every `T` stored in an arena.
const fn assert_storable<T>() {
    assert!(!needs_drop::<T>(), "arena values must not have drop glue");
    assert!(
        align_of::<T>() <= BLOCK_ALIGN,
        "arena values must not be aligned above the block alignment"
    );
}

/// Bump allocator over one block of memory obtained once, at construction.
///
/// Not `Sync`, and not `Send` either (it owns a raw block): one arena stays on
/// the thread that made it. Handing arenas to workers is left to a later card.
#[derive(Debug)]
pub struct Arena {
    block: NonNull<u8>,
    capacity: usize,
    used: Cell<usize>,
}

impl Arena {
    /// An arena with room for `bytes` bytes. Like `Vec::with_capacity`, this
    /// aborts (`handle_alloc_error`) when the system cannot provide the block;
    /// use [`Arena::try_with_capacity`] to get a value instead.
    #[must_use]
    pub fn with_capacity(bytes: usize) -> Self {
        match Self::try_with_capacity(bytes) {
            Some(arena) => arena,
            None => handle_alloc_error(Layout::new::<Self>()),
        }
    }

    /// An arena with room for `bytes` bytes, or `None` when the block cannot be
    /// provided (a size above `isize::MAX` or an allocator failure).
    #[must_use]
    pub fn try_with_capacity(bytes: usize) -> Option<Self> {
        let block = if bytes == 0 {
            NonNull::<u8>::dangling()
        } else {
            let layout = Layout::from_size_align(bytes, BLOCK_ALIGN).ok()?;
            // SAFETY: the layout has a non-zero size.
            NonNull::new(unsafe { std::alloc::alloc(layout) })?
        };
        Some(Self {
            block,
            capacity: bytes,
            used: Cell::new(0),
        })
    }

    /// Total bytes, used or not.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Bytes handed out so far, padding included.
    #[must_use]
    pub fn used(&self) -> usize {
        self.used.get()
    }

    /// Makes the whole block available again. Needs `&mut self`, so the borrow
    /// checker proves that nothing allocated from the arena is still alive.
    pub fn reset(&mut self) {
        self.used.set(0);
    }

    /// Moves `value` into the arena, or returns `None` when it does not fit.
    ///
    /// ```compile_fail,E0080
    /// let arena = base::arena::Arena::with_capacity(64);
    /// let _ = arena.alloc(String::new());
    /// ```
    #[must_use]
    pub fn alloc<T>(&self, value: T) -> Option<ArenaBox<'_, T>> {
        const { assert_storable::<T>() };
        let slot = self.reserve::<T>(1)?;
        // SAFETY: `slot` is aligned, unique and valid for one `T` (see `reserve`).
        unsafe { slot.write(value) };
        Some(ArenaBox {
            slot,
            marker: PhantomData,
        })
    }

    /// A vector of `len` elements `f(0)`, `f(1)`, ... or `None` when they do not fit.
    /// `f` may allocate from this arena again: the space is reserved first. `f`
    /// runs `len` times, also for zero-sized elements.
    #[must_use]
    pub fn alloc_slice_with<T>(
        &self,
        len: usize,
        mut f: impl FnMut(usize) -> T,
    ) -> Option<ArenaVec<'_, T>> {
        let mut vec = self.vec_with_capacity(len)?;
        for index in 0..len {
            // The capacity is `len`, so this cannot hand a value back.
            vec.push(f(index)).ok()?;
        }
        Some(vec)
    }

    /// An empty vector with room for `capacity` elements, or `None` when they do not fit.
    #[must_use]
    pub fn vec_with_capacity<T>(&self, capacity: usize) -> Option<ArenaVec<'_, T>> {
        const { assert_storable::<T>() };
        let start = self.reserve::<T>(capacity)?;
        Some(ArenaVec {
            start,
            len: 0,
            capacity,
            marker: PhantomData,
        })
    }

    /// Reserves aligned, unused space for `count` values of `T`. The returned
    /// pointer is valid for that many `T`s until `reset` or drop, and no other
    /// reservation overlaps it. Zero-sized requests take no space.
    fn reserve<T>(&self, count: usize) -> Option<NonNull<T>> {
        let bytes = size_of::<T>().checked_mul(count)?;
        if bytes == 0 {
            return Some(NonNull::dangling());
        }
        // The block is `BLOCK_ALIGN`-aligned, so aligning the offset aligns the address.
        let offset = self.used.get().checked_next_multiple_of(align_of::<T>())?;
        let end = offset.checked_add(bytes)?;
        if end > self.capacity {
            return None;
        }
        self.used.set(end);
        // SAFETY: `offset + bytes <= capacity`, so the result stays inside the block.
        let slot = unsafe { self.block.as_ptr().add(offset) };
        // SAFETY: `slot` is inside the block, hence not null.
        Some(unsafe { NonNull::new_unchecked(slot) }.cast::<T>())
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        if self.capacity == 0 {
            return;
        }
        // The layout was valid when the block was allocated, with the same arguments.
        if let Ok(layout) = Layout::from_size_align(self.capacity, BLOCK_ALIGN) {
            // SAFETY: `block` came from `alloc` with this layout and is freed only here.
            unsafe { std::alloc::dealloc(self.block.as_ptr(), layout) };
        }
    }
}

/// One value stored in an [`Arena`].
pub struct ArenaBox<'a, T> {
    slot: NonNull<T>,
    marker: PhantomData<&'a mut T>,
}

impl<T: fmt::Debug> fmt::Debug for ArenaBox<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}

impl<T> Deref for ArenaBox<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: `slot` holds an initialised `T` that only this box can reach.
        unsafe { self.slot.as_ref() }
    }
}

impl<T> DerefMut for ArenaBox<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: as above, and `&mut self` makes the access exclusive.
        unsafe { self.slot.as_mut() }
    }
}

/// Push-only vector of fixed capacity stored in an [`Arena`]. It dereferences to a slice.
///
/// A reference kept past `Arena::reset` is rejected at compile time:
///
/// ```compile_fail,E0502
/// let mut arena = base::arena::Arena::with_capacity(64);
/// let list = arena.vec_with_capacity::<u32>(4);
/// arena.reset();
/// drop(list);
/// ```
pub struct ArenaVec<'a, T> {
    start: NonNull<T>,
    len: usize,
    capacity: usize,
    marker: PhantomData<&'a mut [T]>,
}

impl<T> ArenaVec<'_, T> {
    /// Room for this many elements.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Appends `value`, or gives it back when the vector is full.
    ///
    /// # Errors
    /// Returns `Err(value)` when `len() == capacity()`.
    pub fn push(&mut self, value: T) -> Result<(), T> {
        if self.len == self.capacity {
            return Err(value);
        }
        // SAFETY: `len < capacity`, so the slot is inside the reserved space and
        // does not hold a value yet.
        unsafe { self.start.add(self.len).write(value) };
        self.len += 1;
        Ok(())
    }
}

impl<T: fmt::Debug> fmt::Debug for ArenaVec<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T> Deref for ArenaVec<'_, T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        // SAFETY: the first `len` elements are initialised and reachable only through `self`.
        unsafe { core::slice::from_raw_parts(self.start.as_ptr(), self.len) }
    }
}

impl<T> DerefMut for ArenaVec<'_, T> {
    fn deref_mut(&mut self) -> &mut [T] {
        // SAFETY: as above, and `&mut self` makes the access exclusive.
        unsafe { core::slice::from_raw_parts_mut(self.start.as_ptr(), self.len) }
    }
}

#[cfg(test)]
mod tests {
    use super::Arena;
    use core::mem::{align_of, size_of};

    #[repr(align(64))]
    struct Aligned64(u8);

    fn fits<T>(allocation: Option<T>) -> T {
        let Some(value) = allocation else {
            panic!("fits");
        };
        value
    }

    fn address<T>(value: &T) -> usize {
        core::ptr::from_ref(value).addr()
    }

    #[test]
    fn a_value_is_stored_and_read_back() {
        let arena = Arena::with_capacity(64);
        let Some(mut value) = arena.alloc(41_u32) else {
            panic!("fits");
        };
        *value += 1;
        assert_eq!(*value, 42);
    }

    #[test]
    fn capacity_and_used_are_reported() {
        let arena = Arena::with_capacity(128);
        assert_eq!(arena.capacity(), 128);
        assert_eq!(arena.used(), 0);
        let Some(_) = arena.alloc(1_u64) else {
            panic!("fits");
        };
        assert!(arena.used() >= size_of::<u64>());
    }

    #[test]
    fn exact_fit_succeeds_and_one_more_byte_fails() {
        let arena = Arena::with_capacity(8);
        let Some(_) = arena.alloc_slice_with(8, |_| 0_u8) else {
            panic!("8 bytes fit in 8");
        };
        assert!(arena.alloc(1_u8).is_none());
    }

    #[test]
    fn overflow_returns_none_and_leaves_the_arena_usable() {
        let arena = Arena::with_capacity(16);
        assert!(arena.alloc([0_u8; 32]).is_none());
        assert_eq!(arena.used(), 0);
        assert!(arena.alloc(7_u32).is_some());
    }

    #[test]
    fn huge_lengths_return_none_instead_of_overflowing() {
        let arena = Arena::with_capacity(64);
        assert!(arena.alloc_slice_with(usize::MAX, |_| 0_u64).is_none());
        assert!(arena.vec_with_capacity::<u64>(usize::MAX / 2).is_none());
        assert_eq!(arena.used(), 0);
    }

    #[test]
    fn results_are_aligned_in_any_order() {
        let arena = Arena::with_capacity(1024);
        for _ in 0..3 {
            let a = fits(arena.alloc(1_u8));
            let b = fits(arena.alloc(2_u128));
            let c = fits(arena.alloc(3_u16));
            let d = fits(arena.alloc(Aligned64(4)));
            assert_eq!(address(&*a) % align_of::<u8>(), 0);
            assert_eq!(address(&*b) % align_of::<u128>(), 0);
            assert_eq!(address(&*c) % align_of::<u16>(), 0);
            assert_eq!(address(&*d) % 64, 0);
            assert_eq!((*a, *b, *c, d.0), (1, 2, 3, 4));
        }
    }

    #[test]
    fn padding_decides_whether_an_allocation_fits() {
        let arena = Arena::with_capacity(7);
        assert!(arena.alloc(1_u8).is_some());
        assert!(arena.alloc(1_u32).is_none());
        assert_eq!(arena.used(), 1);
        let arena = Arena::with_capacity(8);
        assert!(arena.alloc(1_u8).is_some());
        assert!(arena.alloc(1_u32).is_some());
        assert_eq!(arena.used(), 8);
    }

    #[test]
    fn used_does_not_depend_on_where_the_block_was_placed() {
        let arena = Arena::with_capacity(256);
        let first = fits(arena.alloc(1_u8));
        let second = fits(arena.alloc(Aligned64(2)));
        assert_eq!(address(&*second) - address(&*first), 64);
        assert_eq!(arena.used(), 128);
    }

    #[test]
    fn over_aligned_zero_sized_values_use_no_space() {
        #[repr(align(32))]
        struct Marker;
        let arena = Arena::with_capacity(0);
        assert!(arena.alloc(Marker).is_some());
        assert_eq!(arena.used(), 0);
    }

    #[test]
    fn an_impossible_capacity_is_a_value() {
        assert!(Arena::try_with_capacity(usize::MAX).is_none());
        assert!(Arena::try_with_capacity(0).is_some());
        let arena = fits(Arena::try_with_capacity(32));
        assert_eq!(arena.capacity(), 32);
    }

    #[test]
    fn debug_output_shows_the_contents() {
        let arena = Arena::with_capacity(64);
        let boxed = fits(arena.alloc(7_u8));
        assert_eq!(format!("{boxed:?}"), "7");
        let mut list = fits(arena.vec_with_capacity::<u8>(2));
        assert_eq!(list.push(1), Ok(()));
        assert_eq!(format!("{list:?}"), "[1]");
    }

    #[test]
    fn allocations_do_not_overlap() {
        let arena = Arena::with_capacity(64);
        let (Some(first), Some(second)) =
            (arena.alloc(0xAAAA_AAAA_u32), arena.alloc(0xBBBB_BBBB_u32))
        else {
            panic!("fits");
        };
        assert_ne!(address(&*first), address(&*second));
        assert_eq!((*first, *second), (0xAAAA_AAAA, 0xBBBB_BBBB));
    }

    #[test]
    fn reset_reuses_the_same_memory() {
        let mut arena = Arena::with_capacity(64);
        let first = arena.alloc(1_u64).map(|value| address(&*value));
        assert!(arena.used() > 0);
        arena.reset();
        assert_eq!(arena.used(), 0);
        let again = arena.alloc(2_u64).map(|value| address(&*value));
        assert!(first.is_some());
        assert_eq!(first, again);
    }

    #[test]
    fn a_reset_arena_can_be_filled_again() {
        let mut arena = Arena::with_capacity(16);
        assert!(arena.alloc([1_u8; 16]).is_some());
        assert!(arena.alloc(1_u8).is_none());
        arena.reset();
        assert!(arena.alloc([2_u8; 16]).is_some());
    }

    #[test]
    fn the_closure_can_allocate_from_the_same_arena() {
        let arena = Arena::with_capacity(256);
        let Some(outer) = arena.alloc_slice_with(3, |index| {
            let inner = fits(arena.alloc(index * 10));
            *inner + 1
        }) else {
            panic!("fits");
        };
        assert_eq!(&*outer, &[1, 11, 21]);
    }

    #[test]
    fn slices_hold_what_the_closure_produced() {
        let arena = Arena::with_capacity(64);
        let Some(mut squares) = arena.alloc_slice_with(5, |index| index * index) else {
            panic!("fits");
        };
        assert_eq!(&*squares, &[0, 1, 4, 9, 16]);
        squares[0] = 100;
        assert_eq!(squares[0], 100);
    }

    #[test]
    fn empty_slices_use_no_space() {
        let arena = Arena::with_capacity(8);
        let Some(empty) = arena.alloc_slice_with(0, |_| 0_u64) else {
            panic!("zero elements always fit");
        };
        assert!(empty.is_empty());
        assert_eq!(arena.used(), 0);
    }

    #[test]
    fn zero_sized_values_use_no_space() {
        let arena = Arena::with_capacity(0);
        assert!(arena.alloc(()).is_some());
        let Some(units) = arena.alloc_slice_with(1_000, |_| ()) else {
            panic!("zero-sized elements always fit");
        };
        assert_eq!(units.len(), 1_000);
        assert_eq!(arena.used(), 0);
    }

    #[test]
    fn a_zero_capacity_arena_refuses_everything_but_nothing() {
        let arena = Arena::with_capacity(0);
        assert!(arena.alloc(1_u8).is_none());
    }

    #[test]
    fn a_vec_fills_to_capacity_then_hands_the_value_back() {
        let arena = Arena::with_capacity(64);
        let Some(mut list) = arena.vec_with_capacity::<u16>(3) else {
            panic!("fits");
        };
        assert_eq!(list.capacity(), 3);
        assert!(list.is_empty());
        assert_eq!(list.push(10), Ok(()));
        assert_eq!(list.push(20), Ok(()));
        assert_eq!(list.push(30), Ok(()));
        assert_eq!(list.push(40), Err(40));
        assert_eq!(&*list, &[10, 20, 30]);
    }

    #[test]
    fn a_vec_of_capacity_zero_is_always_full() {
        let arena = Arena::with_capacity(8);
        let Some(mut list) = arena.vec_with_capacity::<u8>(0) else {
            panic!("fits");
        };
        assert_eq!(list.push(1), Err(1));
    }

    #[test]
    fn vec_elements_can_be_changed_through_the_slice() {
        let arena = Arena::with_capacity(64);
        let Some(mut list) = arena.vec_with_capacity::<u32>(4) else {
            panic!("fits");
        };
        assert_eq!(list.push(1), Ok(()));
        assert_eq!(list.push(2), Ok(()));
        list.iter_mut().for_each(|value| *value *= 5);
        assert_eq!(&*list, &[5, 10]);
    }

    #[test]
    fn two_vecs_do_not_overlap() {
        let arena = Arena::with_capacity(64);
        let (Some(mut left), Some(mut right)) = (
            arena.vec_with_capacity::<u32>(2),
            arena.vec_with_capacity::<u32>(2),
        ) else {
            panic!("fits");
        };
        assert_eq!(left.push(1), Ok(()));
        assert_eq!(left.push(2), Ok(()));
        assert_eq!(right.push(3), Ok(()));
        assert_eq!(right.push(4), Ok(()));
        assert_eq!((&*left, &*right), (&[1, 2][..], &[3, 4][..]));
    }
}
