//! The column abstraction of the archetype tables and its safe implementation: a typed `Vec<T>`
//! behind a trait object, so the type is known only at the coarse level of one column.

use std::any::Any;

/// One dense column of one component type, as the archetype tables use it.
pub trait ColumnOps: Sized {
    /// An empty column of component type `T`.
    fn new<T: 'static>() -> Self;
    /// Label of the implementation in the report.
    const LABEL: &'static str;
    /// Number of rows.
    fn len(&self) -> usize;
    /// Bytes of one row.
    fn row_bytes(&self) -> usize;
    /// Whether there is no row.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Appends a value; a value of another type than the column's is dropped.
    fn push<T: 'static>(&mut self, value: T);
    /// The rows as a slice; empty when `T` is not the column's type.
    fn as_slice<T: 'static>(&self) -> &[T];
    /// The rows as a mutable slice; empty when `T` is not the column's type.
    fn as_mut_slice<T: 'static>(&mut self) -> &mut [T];
    /// Moves row `row` to the end of `dst` and fills the hole with the last row.
    fn swap_remove_into(&mut self, row: usize, dst: &mut Self);
    /// Drops row `row` and fills the hole with the last row.
    fn swap_remove_drop(&mut self, row: usize);
}

/// The operations the safe column needs from a typed vector, object safe.
trait ErasedVec {
    fn row_bytes(&self) -> usize;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn len(&self) -> usize;
    fn swap_remove_into(&mut self, row: usize, dst: &mut dyn ErasedVec);
    fn swap_remove_drop(&mut self, row: usize);
}

impl<T: 'static> ErasedVec for Vec<T> {
    fn row_bytes(&self) -> usize {
        std::mem::size_of::<T>()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn len(&self) -> usize {
        Vec::len(self)
    }

    fn swap_remove_into(&mut self, row: usize, dst: &mut dyn ErasedVec) {
        if row >= Vec::len(self) {
            return;
        }
        let value = self.swap_remove(row);
        if let Some(dst) = dst.as_any_mut().downcast_mut::<Vec<T>>() {
            dst.push(value);
        }
    }

    fn swap_remove_drop(&mut self, row: usize) {
        if row < Vec::len(self) {
            drop(self.swap_remove(row));
        }
    }
}

/// A column that stays in safe Rust: a boxed `Vec<T>` found again by `Any::downcast`.
pub struct SafeColumn {
    inner: Box<dyn ErasedVec>,
}

impl ColumnOps for SafeColumn {
    const LABEL: &'static str = "archetype-safe";

    fn row_bytes(&self) -> usize {
        self.inner.row_bytes()
    }

    fn new<T: 'static>() -> Self {
        Self {
            inner: Box::new(Vec::<T>::new()),
        }
    }

    fn len(&self) -> usize {
        self.inner.len()
    }

    fn push<T: 'static>(&mut self, value: T) {
        if let Some(rows) = self.inner.as_any_mut().downcast_mut::<Vec<T>>() {
            rows.push(value);
        }
    }

    fn as_slice<T: 'static>(&self) -> &[T] {
        self.inner
            .as_any()
            .downcast_ref::<Vec<T>>()
            .map_or(&[], Vec::as_slice)
    }

    fn as_mut_slice<T: 'static>(&mut self) -> &mut [T] {
        self.inner
            .as_any_mut()
            .downcast_mut::<Vec<T>>()
            .map_or(&mut [], Vec::as_mut_slice)
    }

    fn swap_remove_into(&mut self, row: usize, dst: &mut Self) {
        self.inner.swap_remove_into(row, dst.inner.as_mut());
    }

    fn swap_remove_drop(&mut self, row: usize) {
        self.inner.swap_remove_drop(row);
    }
}
