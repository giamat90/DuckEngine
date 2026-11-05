//! Three small helpers for the fixed-size tables of the core. The lints ask for checked access and
//! shallow blocks, and a table lookup that may miss is the same three lines everywhere.

/// Stores `value` in `cells[index]`; a missing cell is left alone.
pub(crate) fn put<T>(cells: &mut [T], index: usize, value: T) {
    if let Some(cell) = cells.get_mut(index) {
        *cell = value;
    }
}

/// Applies `change` to `cells[index]`; a missing cell is left alone.
pub(crate) fn change<T>(cells: &mut [T], index: usize, change: impl FnOnce(&mut T)) {
    if let Some(cell) = cells.get_mut(index) {
        change(cell);
    }
}

/// `cells[index]`, or `fallback` for a missing cell.
pub(crate) fn read<T: Copy>(cells: &[T], index: usize, fallback: T) -> T {
    cells.get(index).copied().unwrap_or(fallback)
}

/// A table index as the `u32` the tables keep (the world has at most `MAX_BODIES` slots).
pub(crate) fn index_u32(index: usize) -> u32 {
    u32::try_from(index).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_change_and_read_ignore_a_missing_cell() {
        let mut cells = [1_u32, 2];
        put(&mut cells, 1, 5);
        put(&mut cells, 2, 9);
        change(&mut cells, 0, |c| *c += 10);
        change(&mut cells, 7, |c| *c += 10);
        assert_eq!(cells, [11, 5]);
        assert_eq!(read(&cells, 1, 0), 5);
        assert_eq!(read(&cells, 2, 42), 42);
    }

    #[test]
    fn an_index_that_does_not_fit_is_the_largest_value() {
        assert_eq!(index_u32(7), 7);
        assert_eq!(index_u32(usize::MAX), u32::MAX);
    }
}
