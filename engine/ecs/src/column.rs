//! One dense column of one component type: a typed `Vec<T>` behind a trait object (ADR-0003,
//! no `unsafe`). The type is known only at the coarse level of one column, and found again by
//! `Any::downcast` once per access.

use crate::component::Component;
use crate::error::EcsError;
use std::any::Any;

/// The operations an archetype table needs from a column without knowing its type.
pub(crate) trait Column: Any + Send + Sync {
    /// Drops row `row`; the last row fills the hole.
    fn swap_remove_drop(&mut self, row: usize) -> Result<(), EcsError>;
    /// Moves row `row` to the end of `dst`; the last row fills the hole. Nothing changes when `dst`
    /// holds another type or the row does not exist.
    fn swap_remove_into(&mut self, row: usize, dst: &mut dyn Column) -> Result<(), EcsError>;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Component> Column for Vec<T> {
    fn swap_remove_drop(&mut self, row: usize) -> Result<(), EcsError> {
        if row >= Vec::len(self) {
            return Err(EcsError::StorageMismatch);
        }
        drop(self.swap_remove(row));
        Ok(())
    }

    fn swap_remove_into(&mut self, row: usize, dst: &mut dyn Column) -> Result<(), EcsError> {
        let target = dst
            .as_any_mut()
            .downcast_mut::<Vec<T>>()
            .ok_or(EcsError::StorageMismatch)?;
        if row >= Vec::len(self) {
            return Err(EcsError::StorageMismatch);
        }
        target.push(self.swap_remove(row));
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// An empty column of component type `T`.
pub(crate) fn new_column<T: Component>() -> Box<dyn Column> {
    Box::new(Vec::<T>::new())
}

/// The rows of a column as `Vec<T>`, when it holds `T`.
pub(crate) fn typed<T: Component>(column: &dyn Column) -> Option<&Vec<T>> {
    column.as_any().downcast_ref::<Vec<T>>()
}

/// The rows of a column as a mutable `Vec<T>`, when it holds `T`.
pub(crate) fn typed_mut<T: Component>(column: &mut dyn Column) -> Option<&mut Vec<T>> {
    column.as_any_mut().downcast_mut::<Vec<T>>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column_of(values: &[u32]) -> Box<dyn Column> {
        Box::new(values.to_vec())
    }

    fn rows(column: &dyn Column) -> Vec<u32> {
        typed::<u32>(column).cloned().unwrap_or_default()
    }

    #[test]
    fn swap_remove_into_moves_the_row_and_fills_the_hole_with_the_last() {
        let mut src = column_of(&[10, 20, 30]);
        let mut dst = column_of(&[]);
        assert_eq!(src.swap_remove_into(0, dst.as_mut()), Ok(()));
        assert_eq!(rows(src.as_ref()), vec![30, 20]);
        assert_eq!(rows(dst.as_ref()), vec![10]);
        assert_eq!((rows(src.as_ref()).len(), rows(dst.as_ref()).len()), (2, 1));
    }

    #[test]
    fn swap_remove_into_the_last_row_needs_no_fill() {
        let mut src = column_of(&[1, 2]);
        let mut dst = column_of(&[9]);
        assert_eq!(src.swap_remove_into(1, dst.as_mut()), Ok(()));
        assert_eq!(rows(src.as_ref()), vec![1]);
        assert_eq!(rows(dst.as_ref()), vec![9, 2]);
    }

    #[test]
    fn swap_remove_into_a_column_of_another_type_changes_nothing() {
        let mut src = column_of(&[1, 2]);
        let mut dst: Box<dyn Column> = Box::new(Vec::<u8>::new());
        assert_eq!(
            src.swap_remove_into(0, dst.as_mut()),
            Err(EcsError::StorageMismatch)
        );
        assert_eq!(rows(src.as_ref()), vec![1, 2]);
        assert_eq!(rows(dst.as_ref()).len(), 0);
    }

    #[test]
    fn swap_remove_into_a_missing_row_changes_nothing() {
        let mut src = column_of(&[1]);
        let mut dst = column_of(&[]);
        assert_eq!(
            src.swap_remove_into(1, dst.as_mut()),
            Err(EcsError::StorageMismatch)
        );
        assert_eq!((rows(src.as_ref()).len(), rows(dst.as_ref()).len()), (1, 0));
    }

    #[test]
    fn swap_remove_drop_removes_one_row_and_refuses_a_missing_one() {
        let mut column = column_of(&[1, 2, 3]);
        assert_eq!(column.swap_remove_drop(0), Ok(()));
        assert_eq!(rows(column.as_ref()), vec![3, 2]);
        assert_eq!(column.swap_remove_drop(2), Err(EcsError::StorageMismatch));
        assert_eq!(rows(column.as_ref()).len(), 2);
    }

    #[test]
    fn typed_access_needs_the_right_type() {
        let mut column = new_column::<u32>();
        assert!(typed::<u8>(column.as_ref()).is_none());
        assert!(typed_mut::<u8>(column.as_mut()).is_none());
        assert!(typed_mut::<u32>(column.as_mut()).is_some());
    }
}
