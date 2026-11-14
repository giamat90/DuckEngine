//! The type-erased column: values stay in order across pushes and moves, drop glue runs exactly
//! once per value, and a wrong type is refused instead of reinterpreted.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tools_ecs_spike::column::ColumnOps;
use tools_ecs_spike::raw_column::RawColumn;

struct Counted {
    id: u32,
    drops: Arc<AtomicUsize>,
}

impl Drop for Counted {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

fn counted(id: u32, drops: &Arc<AtomicUsize>) -> Counted {
    Counted {
        id,
        drops: Arc::clone(drops),
    }
}

fn ids(column: &RawColumn) -> Vec<u32> {
    column.as_slice::<Counted>().iter().map(|c| c.id).collect()
}

#[test]
fn pushed_values_read_back_in_order_across_growth() {
    let mut column = RawColumn::new::<u64>();
    for value in 0..100_u64 {
        column.push(value);
    }
    assert_eq!(column.len(), 100);
    assert_eq!(column.as_slice::<u64>(), (0..100).collect::<Vec<u64>>());
    for value in column.as_mut_slice::<u64>() {
        *value += 1;
    }
    assert_eq!(column.as_slice::<u64>().first(), Some(&1));
}

#[test]
fn a_wrong_type_is_refused() {
    let mut column = RawColumn::new::<u32>();
    column.push(7_u32);
    column.push(1.5_f32);
    column.push(9_u64);
    assert_eq!(column.len(), 1);
    assert!(column.as_slice::<f32>().is_empty());
    assert!(column.as_mut_slice::<u64>().is_empty());
    assert_eq!(column.as_slice::<u32>(), [7]);
}

#[test]
fn swap_remove_into_moves_the_value_and_fills_the_hole_with_the_last() {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut from = RawColumn::new::<Counted>();
    let mut to = RawColumn::new::<Counted>();
    for id in 0..5 {
        from.push(counted(id, &drops));
    }
    from.swap_remove_into(1, &mut to);
    assert_eq!(ids(&from), [0, 4, 2, 3]);
    assert_eq!(ids(&to), [1]);
    from.swap_remove_into(3, &mut to);
    assert_eq!(ids(&from), [0, 4, 2]);
    assert_eq!(ids(&to), [1, 3]);
    assert_eq!(drops.load(Ordering::SeqCst), 0, "a move never drops");
}

#[test]
fn swap_remove_drop_drops_exactly_that_value() {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut column = RawColumn::new::<Counted>();
    for id in 0..4 {
        column.push(counted(id, &drops));
    }
    column.swap_remove_drop(0);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(ids(&column), [3, 1, 2]);
    column.swap_remove_drop(2);
    assert_eq!(drops.load(Ordering::SeqCst), 2);
    assert_eq!(ids(&column), [3, 1]);
    column.swap_remove_drop(9);
    assert_eq!(
        drops.load(Ordering::SeqCst),
        2,
        "an out of range row does nothing"
    );
}

#[test]
fn dropping_the_column_drops_every_remaining_value_once() {
    let drops = Arc::new(AtomicUsize::new(0));
    {
        let mut column = RawColumn::new::<Counted>();
        for id in 0..10 {
            column.push(counted(id, &drops));
        }
        column.swap_remove_drop(3);
    }
    assert_eq!(drops.load(Ordering::SeqCst), 10);
}

#[test]
fn moving_between_columns_of_different_types_does_nothing() {
    let mut ints = RawColumn::new::<u32>();
    let mut floats = RawColumn::new::<f32>();
    ints.push(1_u32);
    ints.swap_remove_into(0, &mut floats);
    assert_eq!(ints.len(), 1);
    assert!(floats.is_empty());
}

#[test]
fn a_zero_sized_type_counts_rows_without_allocating() {
    let mut column = RawColumn::new::<()>();
    for _ in 0..1_000 {
        column.push(());
    }
    assert_eq!(column.len(), 1_000);
    assert_eq!(column.as_slice::<()>().len(), 1_000);
    column.swap_remove_drop(10);
    assert_eq!(column.len(), 999);
    let mut other = RawColumn::new::<()>();
    column.swap_remove_into(0, &mut other);
    assert_eq!((column.len(), other.len()), (998, 1));
}

#[test]
fn alignment_is_kept_for_a_wide_type() {
    #[repr(align(32))]
    #[derive(Clone, Copy, PartialEq, Debug)]
    struct Wide(u8);
    let mut column = RawColumn::new::<Wide>();
    for value in 0..9_u8 {
        column.push(Wide(value));
    }
    for (row, value) in column.as_slice::<Wide>().iter().enumerate() {
        assert_eq!((std::ptr::from_ref(value) as usize) % 32, 0, "row {row}");
    }
    assert_eq!(column.as_slice::<Wide>().get(8), Some(&Wide(8)));
}
