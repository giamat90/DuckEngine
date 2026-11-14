//! The type-erased column: raw bytes plus the layout, the type id and the drop glue of one
//! component type. This is the only `unsafe` of the crate; it is what an archetype table needs
//! when components are registered at run time and rows move between tables as plain bytes.
//!
//! Invariants (the protocol every block below relies on):
//! 1. `ptr` is valid for `cap` elements of `layout` (for a zero-sized type it is a dangling,
//!    aligned pointer and `cap` is `usize::MAX`, nothing is ever allocated);
//! 2. the first `len` elements are initialised, the rest are not;
//! 3. every typed access first compares `TypeId::of::<T>()` with `type_id`, so `T` has exactly
//!    the size and alignment of `layout`;
//!    (A panic in a destructor during `swap_remove_drop` would leave invariant 2 broken; the
//!    workspace builds with `panic = abort`, so this is not handled.)
//! 4. an element copied out by a swap-remove is owned by the destination from then on and is not
//!    dropped in the source.

use crate::column::ColumnOps;
use std::alloc::{Layout, alloc, dealloc, handle_alloc_error, realloc};
use std::any::TypeId;
use std::ptr::{self, NonNull};

/// First capacity of a column that grows.
const FIRST_CAPACITY: usize = 4;

/// A column of one component type whose type is known only at run time.
pub struct RawColumn {
    ptr: NonNull<u8>,
    len: usize,
    cap: usize,
    layout: Layout,
    type_id: TypeId,
    drop_rows: Option<unsafe fn(*mut u8, usize)>,
}

/// Drops `len` elements of type `T` starting at `ptr`.
///
/// # Safety
/// `ptr` must point to `len` initialised, aligned elements of `T` that nobody uses afterwards.
unsafe fn drop_rows_of<T>(ptr: *mut u8, len: usize) {
    // SAFETY: the caller guarantees `len` initialised `T` at `ptr`, not used again.
    unsafe { ptr::drop_in_place(ptr::slice_from_raw_parts_mut(ptr.cast::<T>(), len)) }
}

impl RawColumn {
    fn element_size(&self) -> usize {
        self.layout.size()
    }

    fn same_type(&self, other: &Self) -> bool {
        self.type_id == other.type_id
    }

    /// Makes room for one more element.
    fn reserve_one(&mut self) {
        if self.len < self.cap {
            return;
        }
        let new_cap = if self.cap == 0 {
            FIRST_CAPACITY
        } else {
            self.cap.saturating_mul(2)
        };
        let Some(new_layout) = self.array_layout(new_cap) else {
            handle_alloc_error(self.layout)
        };
        let Some(ptr) = NonNull::new(self.allocate_or_grow(new_layout)) else {
            handle_alloc_error(new_layout)
        };
        self.ptr = ptr;
        self.cap = new_cap;
    }

    /// The layout of `cap` elements; `None` when the size overflows.
    fn array_layout(&self, cap: usize) -> Option<Layout> {
        let bytes = self.element_size().checked_mul(cap)?;
        Layout::from_size_align(bytes, self.layout.align()).ok()
    }

    /// A block for `new_layout`: fresh when the column has none, else the current one grown.
    fn allocate_or_grow(&self, new_layout: Layout) -> *mut u8 {
        if self.cap == 0 {
            // SAFETY: `new_layout` has a non-zero size (`element size > 0` because a zero-sized
            // column has `cap == usize::MAX` and never gets here, and the new capacity is >= 1).
            return unsafe { alloc(new_layout) };
        }
        let Some(old_layout) = self.array_layout(self.cap) else {
            handle_alloc_error(self.layout)
        };
        // SAFETY: `ptr` was allocated with `old_layout` (the layout of `cap` elements, as at the
        // last allocation) and the new size is non-zero and valid for the alignment (checked
        // by `Layout::from_size_align` in `array_layout`).
        unsafe { realloc(self.ptr.as_ptr(), old_layout, new_layout.size()) }
    }

    /// Fills the hole at `row` with the last element and shortens the column by one. The element
    /// at `row` must already be moved out or dropped.
    fn fill_hole(&mut self, row: usize) {
        let last = self.len - 1;
        let size = self.element_size();
        if row != last && size != 0 {
            // SAFETY: `row < last < len`, both elements are in the allocation, they are distinct
            // so the ranges do not overlap, and the source is initialised.
            unsafe {
                let base = self.ptr.as_ptr();
                ptr::copy_nonoverlapping(base.add(last * size), base.add(row * size), size);
            }
        }
        self.len = last;
    }
}

impl ColumnOps for RawColumn {
    const LABEL: &'static str = "archetype-raw";

    fn row_bytes(&self) -> usize {
        self.element_size()
    }

    fn new<T: 'static>() -> Self {
        let layout = Layout::new::<T>();
        Self {
            ptr: NonNull::<T>::dangling().cast::<u8>(),
            len: 0,
            cap: if layout.size() == 0 { usize::MAX } else { 0 },
            layout,
            type_id: TypeId::of::<T>(),
            drop_rows: std::mem::needs_drop::<T>().then_some(drop_rows_of::<T> as unsafe fn(_, _)),
        }
    }

    fn len(&self) -> usize {
        self.len
    }

    fn push<T: 'static>(&mut self, value: T) {
        if TypeId::of::<T>() != self.type_id {
            return;
        }
        self.reserve_one();
        // SAFETY: the type id matches (invariant 3), so `T` has the layout of the column;
        // `reserve_one` made `len < cap`, so slot `len` is inside the allocation and unused.
        unsafe { self.ptr.as_ptr().cast::<T>().add(self.len).write(value) };
        self.len += 1;
    }

    fn as_slice<T: 'static>(&self) -> &[T] {
        if TypeId::of::<T>() != self.type_id {
            return &[];
        }
        // SAFETY: the type id matches (invariant 3), `ptr` is aligned for `T`, the first `len`
        // elements are initialised (invariant 2) and `&self` forbids a mutable alias.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr().cast::<T>(), self.len) }
    }

    fn as_mut_slice<T: 'static>(&mut self) -> &mut [T] {
        if TypeId::of::<T>() != self.type_id {
            return &mut [];
        }
        // SAFETY: as in `as_slice`, and `&mut self` makes the slice the only access.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr().cast::<T>(), self.len) }
    }

    fn swap_remove_into(&mut self, row: usize, dst: &mut Self) {
        if row >= self.len || !self.same_type(dst) {
            return;
        }
        dst.reserve_one();
        let size = self.element_size();
        if size != 0 {
            // SAFETY: `row < len` so the source element is initialised; `dst` has room for one
            // more (`reserve_one`); the two columns are distinct allocations (`&mut` on both) so
            // the ranges do not overlap; both have the same element layout (same type id).
            unsafe {
                let from = self.ptr.as_ptr().add(row * size);
                let to = dst.ptr.as_ptr().add(dst.len * size);
                ptr::copy_nonoverlapping(from, to, size);
            }
        }
        dst.len += 1;
        self.fill_hole(row);
    }

    fn swap_remove_drop(&mut self, row: usize) {
        if row >= self.len {
            return;
        }
        if let Some(drop_rows) = self.drop_rows {
            // SAFETY: `row < len`, so one initialised element of the column's type is at that
            // address; it is overwritten (or forgotten, if last) right after by `fill_hole`.
            unsafe { drop_rows(self.ptr.as_ptr().add(row * self.element_size()), 1) };
        }
        self.fill_hole(row);
    }
}

impl Drop for RawColumn {
    fn drop(&mut self) {
        if let Some(drop_rows) = self.drop_rows {
            // SAFETY: the first `len` elements are initialised (invariant 2) and the column is
            // being destroyed, so nothing uses them again.
            unsafe { drop_rows(self.ptr.as_ptr(), self.len) };
        }
        // A column that never allocated (`cap == 0`) or holds a zero-sized type has a zero size.
        let layout = self
            .array_layout(self.cap)
            .filter(|layout| layout.size() != 0);
        if let Some(layout) = layout {
            // SAFETY: `ptr` was allocated with exactly this layout (`cap` elements, same
            // alignment) and is freed once.
            unsafe { dealloc(self.ptr.as_ptr(), layout) };
        }
    }
}
